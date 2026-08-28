//! `ds mcp install` — the host entry that launches `ds mcp serve`.
//!
//! Every MCP host reads the same shape: a named stdio server with a command
//! and arguments. What differs is the file it lives in. This command prints
//! the entry for this executable and, with `--write`, merges it into the
//! host's user-level file (never a workspace file: the server must run on the
//! machine that has DS GridDesign, and a workspace file travels).
//!
//! That target is what fixes the effect class. A user-level host
//! configuration is not "a file in your workspace" — it changes how every
//! agent session on this machine starts, and it survives the workspace being
//! deleted. So the command is `machine_write` and dispatch requires `--yes`,
//! the same gate `ds workstation install` passes through. It was
//! `local_file_write` once, which is not in the confirmation set, so the
//! `--yes` this command's own help asked for was decorative.

use std::fs::{File, Metadata, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

pub const HOSTS: &[&str] = &["vscode", "claude-code", "codex", "cursor", "generic"];

pub static COMMAND: Command = Command {
    id: "mcp.install",
    path: &["mcp", "install"],
    contract: 2,
    chapter: ds_cli_contract::spec::Chapter::Catalog,
    summary: "Print or write the MCP host entry that launches this `ds`.",
    purpose: "\
Prints this executable's stdio server entry and its user-level host file. With \
`--write`, merges only the `ds` entry and preserves other servers, staging the \
merged document beside the target and renaming it into place. It never writes \
workspace configuration. Changing a user-level host file changes how agent \
sessions start on this machine, so every invocation needs `--yes`.",
    effect: Effect::MachineWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg {
            name: "host",
            kind: ArgKind::Value,
            value: "<vscode|claude-code|codex|cursor|generic>",
            required: false,
            default: Some("vscode"),
            choices: &["vscode", "claude-code", "codex", "cursor", "generic"],
            summary: "Which host's configuration shape and file to target.",
        },
        Arg {
            name: "write",
            kind: ArgKind::Switch,
            value: "",
            required: false,
            default: None,
            choices: &[],
            summary: "Merge the entry into the host's user-level file instead of only printing it.",
        },
        Arg {
            name: "exposure",
            kind: ArgKind::Value,
            value: "<chapters|commands>",
            required: false,
            default: Some("chapters"),
            choices: crate::surface::EXPOSURES,
            summary: "Publish compact chapter routers or typed command tools.",
        },
        Arg {
            name: "profile",
            kind: ArgKind::Value,
            value: "<name>",
            required: false,
            default: None,
            choices: crate::surface::PROFILE_IDS,
            summary: "Install one typed operator-workflow profile.",
        },
    ],
    output: "\
The host, the server entry as JSON, the file it belongs in, and whether it \
was written.",
    examples: &[
        Example {
            command: "ds mcp install --output json --yes",
            note: "Print the VS Code entry and its file path; without `--write` nothing is written.",
            runnable: true,
        },
        Example {
            command: "ds mcp install --host vscode --write --yes",
            note: "Merge the `ds` server into the VS Code user profile's mcp.json.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::HOST_UNKNOWN,
        crate::CONFIG_UNWRITABLE,
        crate::PROFILE_EXPOSURE_INVALID,
        crate::CAPABILITIES_UNAVAILABLE,
        Refusal {
            code: "confirmation_required",
            when: "--yes was not given for a command that can change this machine's host configuration",
            remedy: "re-run with --yes; without --write the entry is only printed",
        },
    ],
    reference: Some("docs/reference/mcp.md"),
    availability: crate::always,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let host = inputs.value("host").unwrap_or("vscode");
    if !HOSTS.contains(&host) {
        return Err(Failure::invalid("mcp_host_unknown", format!("no configuration recipe for host `{host}`"))
            .remedy("pass one of the hosts listed in `ds mcp install --help`, or omit --host to print the generic entry")
            .detail(json!({ "hosts": HOSTS })));
    }
    let executable = std::env::current_exe().map_err(|error| {
        Failure::failed("mcp_config_unwritable", format!("could not resolve this executable's path: {error}"))
            .remedy("read the reported path; fix or remove a malformed file, then re-run, or copy the printed entry by hand")
    })?;
    let build = crate::tools::build_identity(&executable)?;
    let exposure =
        crate::surface::Exposure::from_token(inputs.value("exposure").unwrap_or("chapters"))
            .expect("the command parser enforces exposure choices");
    let profile = inputs.value("profile").map(|value| {
        crate::surface::Profile::from_token(value)
            .expect("the command parser enforces profile choices")
    });
    if profile.is_some() && exposure != crate::surface::Exposure::Commands {
        return Err(Failure::invalid(
            "mcp_profile_exposure_invalid",
            "specialized profiles publish typed command tools and require `--exposure commands`",
        )
        .remedy("pass `--exposure commands --profile <name>`, or omit `--profile`"));
    }
    let entry = server_entry(host, &executable, exposure, profile);
    let file = config_file(host);
    let written = if inputs.switch("write") {
        let Some(path) = file.as_ref() else {
            return Err(Failure::invalid("mcp_host_unknown", format!("host `{host}` has no user-level file to write; copy the printed entry into your host's MCP settings"))
                .remedy("pass one of the hosts listed in `ds mcp install --help`, or omit --host to print the generic entry"));
        };
        merge_into_file(path, host, &entry)?;
        true
    } else {
        false
    };
    Ok(json!({
        "host": host,
        "server_name": "ds",
        "entry": entry,
        "file": file.map(|path| path.display().to_string()),
        "written": written,
        "executable": executable.display().to_string(),
        "source_sha": build["source_sha"],
        "dirty": build["dirty"],
        "exposure": exposure.token(),
        "profile": profile.map(crate::surface::Profile::token),
    }))
}

pub fn render(data: &Value) -> String {
    let mut out = String::new();
    out.push_str(&format!("host: {}\n", data["host"].as_str().unwrap_or("?")));
    if let Some(file) = data["file"].as_str() {
        out.push_str(&format!(
            "file: {file}{}\n",
            if data["written"].as_bool().unwrap_or(false) {
                "  (written)"
            } else {
                ""
            }
        ));
    }
    out.push_str("entry:\n");
    out.push_str(&serde_json::to_string_pretty(&data["entry"]).unwrap_or_default());
    out.push('\n');
    out
}

/// The entry in the host's own dialect. Every host launches the same thing.
pub fn server_entry(
    host: &str,
    executable: &std::path::Path,
    exposure: crate::surface::Exposure,
    profile: Option<crate::surface::Profile>,
) -> Value {
    let command = executable.display().to_string();
    let mut args = vec!["mcp", "serve", "--exposure", exposure.token()];
    if let Some(profile) = profile {
        args.extend(["--profile", profile.token()]);
    }
    match host {
        // VS Code: `servers` map with an explicit `type`.
        "vscode" => {
            json!({ "servers": { "ds": { "type": "stdio", "command": command, "args": args } } })
        }
        // Claude Code, Cursor: `mcpServers` map.
        "claude-code" | "cursor" | "generic" => {
            json!({ "mcpServers": { "ds": { "command": command, "args": args } } })
        }
        // Codex: TOML on disk; the JSON here is the same data for the caller to translate.
        "codex" => {
            json!({ "mcp_servers": { "ds": { "command": command, "args": args } } })
        }
        _ => json!({ "mcpServers": { "ds": { "command": command, "args": args } } }),
    }
}

/// The user-level file for the host on this platform; `None` when the host
/// keeps no JSON file this command should touch (Codex uses TOML).
pub fn config_file(host: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    match host {
        "vscode" => Some(if cfg!(target_os = "windows") {
            PathBuf::from(std::env::var_os("APPDATA")?)
                .join("Code")
                .join("User")
                .join("mcp.json")
        } else if cfg!(target_os = "macos") {
            home.join("Library")
                .join("Application Support")
                .join("Code")
                .join("User")
                .join("mcp.json")
        } else {
            home.join(".config")
                .join("Code")
                .join("User")
                .join("mcp.json")
        }),
        "claude-code" => Some(home.join(".claude.json")),
        "cursor" => Some(home.join(".cursor").join("mcp.json")),
        _ => None,
    }
}

/// Merge `entry` (one top-level map with one server) into the JSON at `path`,
/// keeping every other key and server. A malformed file is refused, never
/// overwritten.
///
/// The target-file policy is deliberately conservative: a target, its direct
/// parent, the lock, the staging files, and the backup must all be ordinary
/// files/directories, never symlinks or Windows reparse points. In particular
/// this command never follows a target symlink and replaces its referent.
///
/// A bounded, same-directory exclusive lock serializes cooperating DS writers.
/// We also re-read the target immediately before replacement, so an external
/// editor that did not take that lock is preserved and the install is refused
/// instead of silently discarding its change. The stage and backup are synced
/// before rename, and the parent directory is synced where the platform
/// supports directory fsync.
pub fn merge_into_file(path: &Path, host: &str, entry: &Value) -> Result<(), Failure> {
    merge_into_file_with_hook(path, host, entry, || {})
}

fn merge_into_file_with_hook(
    path: &Path,
    host: &str,
    entry: &Value,
    before_replace: impl FnOnce(),
) -> Result<(), Failure> {
    let unwritable = |message: String| {
        Failure::failed("mcp_config_unwritable", message)
            .remedy("read the reported path; fix or remove a malformed file, then re-run, or copy the printed entry by hand")
            .detail(json!({ "file": path.display().to_string(), "host": host }))
    };
    let parent = prepare_parent(path)
        .map_err(|error| unwritable(format!("could not prepare {}: {error}", path.display())))?;
    let _lock = InstallLock::acquire(path)
        .map_err(|error| unwritable(format!("could not lock {}: {error}", path.display())))?;

    let existing = read_regular_file(path)
        .map_err(|error| unwritable(format!("could not read {}: {error}", path.display())))?;
    let document: Value = match existing.as_ref().map(|existing| existing.bytes.as_slice()) {
        None => json!({}),
        Some(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => json!({}),
        Some(bytes) => serde_json::from_slice(bytes).map_err(|error| {
            unwritable(format!("{} is not valid JSON: {error}", path.display()))
        })?,
    };
    let document = merge(&document, entry).ok_or_else(|| {
        unwritable(format!(
            "{} does not hold an object at its root",
            path.display()
        ))
    })?;
    let mut bytes =
        serde_json::to_vec_pretty(&document).map_err(|error| unwritable(error.to_string()))?;
    bytes.push(b'\n');

    // Same directory, so replacement cannot cross a filesystem boundary and
    // degrade into a copy. `create_new` makes a hostile pre-created stage a
    // refusal rather than a truncation or a followed symlink.
    let mut staged = StagedFile::create(path, "stage").map_err(|error| {
        unwritable(format!(
            "could not create private stage for {}: {error}",
            path.display()
        ))
    })?;
    staged
        .write_synced(&bytes, existing.as_ref().map(|existing| &existing.metadata))
        .map_err(|error| {
            unwritable(format!(
                "could not write {}: {error}",
                staged.path.display()
            ))
        })?;

    if let Some(previous) = &existing {
        write_backup(path, previous).map_err(|error| {
            unwritable(format!(
                "could not preserve backup for {}: {error}",
                path.display()
            ))
        })?;
    }

    // The lock protects DS installers. This check covers an editor or another
    // program that changes the path without joining that protocol.
    before_replace();
    let current = read_regular_file(path).map_err(|error| {
        unwritable(format!(
            "could not re-check {} before replacement: {error}",
            path.display()
        ))
    })?;
    if !same_contents(existing.as_ref(), current.as_ref()) {
        return Err(unwritable(format!(
            "{} changed while its replacement was being prepared; it was left untouched",
            path.display()
        )));
    }

    staged
        .replace(path)
        .map_err(|error| unwritable(format!("could not replace {}: {error}", path.display())))?;
    sync_parent(parent).map_err(|error| {
        unwritable(format!(
            "could not make {} durable: {error}",
            parent.display()
        ))
    })
}

const LOCK_ATTEMPTS: u32 = 100;
const LOCK_WAIT: Duration = Duration::from_millis(20);
static PRIVATE_PATH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn sibling_path(path: &Path, kind: &str) -> PathBuf {
    let name = path.file_name().map_or_else(
        || String::from("mcp.json"),
        |name| name.to_string_lossy().into_owned(),
    );
    let sequence = PRIVATE_PATH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!(
        ".{name}.ds-{kind}-{}-{sequence}.tmp",
        std::process::id()
    ))
}

fn lock_path(path: &Path) -> PathBuf {
    let name = path.file_name().map_or_else(
        || String::from("mcp.json"),
        |name| name.to_string_lossy().into_owned(),
    );
    path.with_file_name(format!(".{name}.ds.lock"))
}

fn backup_path(path: &Path) -> PathBuf {
    let name = path.file_name().map_or_else(
        || String::from("mcp.json"),
        |name| name.to_string_lossy().into_owned(),
    );
    path.with_file_name(format!("{name}.bak"))
}

struct ExistingFile {
    bytes: Vec<u8>,
    metadata: Metadata,
}

fn same_contents(left: Option<&ExistingFile>, right: Option<&ExistingFile>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => left.bytes == right.bytes,
        _ => false,
    }
}

fn prepare_parent(path: &Path) -> std::io::Result<&Path> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target has no parent directory",
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let metadata = std::fs::symlink_metadata(parent)?;
    if metadata.file_type().is_symlink() || is_reparse_point(&metadata) || !metadata.is_dir() {
        return Err(unsafe_path_error(parent));
    }
    Ok(parent)
}

fn read_regular_file(path: &Path) -> std::io::Result<Option<ExistingFile>> {
    let checked = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if checked.file_type().is_symlink() || is_reparse_point(&checked) || !checked.is_file() {
        return Err(unsafe_path_error(path));
    }
    let mut file = open_read_no_follow(path)?;
    let opened = file.metadata()?;
    if is_reparse_point(&opened) || !opened.is_file() {
        return Err(unsafe_path_error(path));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(Some(ExistingFile {
        bytes,
        // Preserve the policy of the inode actually opened, not a pathname
        // observation that might have changed before O_NOFOLLOW opened it.
        metadata: opened,
    }))
}

fn unsafe_path_error(path: &Path) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!(
            "{} is not an ordinary non-link file or directory",
            path.display()
        ),
    )
}

#[cfg(windows)]
fn is_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &Metadata) -> bool {
    false
}

#[cfg(unix)]
fn open_read_no_follow(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(windows)]
fn open_read_no_follow(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt as _;
    // FILE_FLAG_OPEN_REPARSE_POINT makes CreateFile open the link object rather
    // than its referent; reading it then fails, which is the desired refusal.
    OpenOptions::new()
        .read(true)
        .custom_flags(0x0020_0000)
        .open(path)
}

#[cfg(not(any(unix, windows)))]
fn open_read_no_follow(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().read(true).open(path)
}

fn create_exclusive(path: &Path) -> std::io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        return OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(0x0020_0000)
            .open(path);
    }
    #[cfg(not(any(unix, windows)))]
    OpenOptions::new().write(true).create_new(true).open(path)
}

fn preserve_file_policy(file: &File, source: Option<&Metadata>) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd as _;
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        if let Some(source) = source {
            // Rename replaces ownership with that of the staging file. Preserve
            // owner/group when the platform lets this process do so, rather than
            // silently changing the access policy of a secret-bearing config.
            if unsafe { libc::fchown(file.as_raw_fd(), source.uid(), source.gid()) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            return file.set_permissions(std::fs::Permissions::from_mode(source.mode() & 0o7777));
        }
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        if let Some(source) = source {
            return file.set_permissions(source.permissions());
        }
        Ok(())
    }
}

fn sync_parent(parent: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        File::open(parent)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        // Windows does not expose a portable directory handle sync through
        // std. The staged file itself is flushed before replacement.
        let _ = parent;
        Ok(())
    }
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

struct StagedFile {
    path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl StagedFile {
    fn create(target: &Path, kind: &str) -> std::io::Result<Self> {
        for _ in 0..32 {
            let path = sibling_path(target, kind);
            match create_exclusive(&path) {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                        committed: false,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not reserve a private staging path",
        ))
    }

    fn write_synced(&mut self, bytes: &[u8], policy: Option<&Metadata>) -> std::io::Result<()> {
        let file = self.file.as_mut().expect("stage is open until replacement");
        file.write_all(bytes)?;
        preserve_file_policy(file, policy)?;
        file.sync_all()
    }

    fn replace(mut self, destination: &Path) -> std::io::Result<()> {
        drop(self.file.take());
        atomic_replace(&self.path, destination)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn write_backup(path: &Path, previous: &ExistingFile) -> std::io::Result<()> {
    let backup = backup_path(path);
    if let Some(metadata) = read_path_metadata(&backup)?
        && (metadata.file_type().is_symlink() || is_reparse_point(&metadata) || !metadata.is_file())
    {
        return Err(unsafe_path_error(&backup));
    }
    let parent = backup.parent().ok_or_else(|| unsafe_path_error(&backup))?;
    let mut staged = StagedFile::create(&backup, "backup")?;
    staged.write_synced(&previous.bytes, Some(&previous.metadata))?;
    staged.replace(&backup)?;
    sync_parent(parent)
}

fn read_path_metadata(path: &Path) -> std::io::Result<Option<Metadata>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

struct InstallLock {
    path: PathBuf,
    token: Vec<u8>,
    _file: File,
}

impl InstallLock {
    fn acquire(target: &Path) -> std::io::Result<Self> {
        Self::acquire_with_attempts(target, LOCK_ATTEMPTS)
    }

    fn acquire_with_attempts(target: &Path, attempts: u32) -> std::io::Result<Self> {
        let path = lock_path(target);
        for attempt in 0..attempts {
            match create_exclusive(&path) {
                Ok(mut file) => {
                    let token = format!(
                        "ds mcp install {} {}\n",
                        std::process::id(),
                        PRIVATE_PATH_SEQUENCE.fetch_add(1, Ordering::Relaxed)
                    )
                    .into_bytes();
                    file.write_all(&token)?;
                    file.sync_all()?;
                    return Ok(Self {
                        path,
                        token,
                        _file: file,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let metadata = read_path_metadata(&path)?
                        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::WouldBlock))?;
                    if metadata.file_type().is_symlink()
                        || is_reparse_point(&metadata)
                        || !metadata.is_file()
                    {
                        return Err(unsafe_path_error(&path));
                    }
                    if attempt + 1 == attempts {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::WouldBlock,
                            "another DS installer is still updating this file; retry after it finishes",
                        ));
                    }
                    std::thread::sleep(LOCK_WAIT);
                }
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "lock attempts must be non-zero",
        ))
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        // Never remove a hostile replacement. The contents check is also what
        // makes a left-over lock inspectable and safely recoverable by a user.
        if let Ok(Some(metadata)) = read_path_metadata(&self.path)
            && metadata.is_file()
            && !metadata.file_type().is_symlink()
            && !is_reparse_point(&metadata)
            && std::fs::read(&self.path).ok().as_deref() == Some(self.token.as_slice())
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Pure merge: the entry's single top-level map key is merged into the
/// document's map of the same name, replacing only the `ds` server.
pub fn merge(document: &Value, entry: &Value) -> Option<Value> {
    let mut root = document.as_object()?.clone();
    for (section, servers) in entry.as_object()? {
        let mut existing = root
            .get(section)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_else(Map::new);
        for (name, server) in servers.as_object()? {
            existing.insert(name.clone(), server.clone());
        }
        root.insert(section.clone(), Value::Object(existing));
    }
    Some(Value::Object(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_entry(host: &str, executable: &std::path::Path) -> Value {
        server_entry(host, executable, crate::surface::Exposure::Chapters, None)
    }

    #[test]
    fn vscode_entry_is_a_stdio_server_pointing_at_this_executable() {
        let entry = default_entry("vscode", std::path::Path::new("/usr/bin/ds"));
        assert_eq!(entry["servers"]["ds"]["type"], "stdio");
        assert_eq!(entry["servers"]["ds"]["command"], "/usr/bin/ds");
        assert_eq!(
            entry["servers"]["ds"]["args"],
            json!(["mcp", "serve", "--exposure", "chapters"])
        );
    }

    #[test]
    fn typed_profile_entry_names_both_exposure_and_profile() {
        let entry = server_entry(
            "claude-code",
            std::path::Path::new("/usr/bin/ds"),
            crate::surface::Exposure::Commands,
            Some(crate::surface::Profile::Pls),
        );
        assert_eq!(
            entry["mcpServers"]["ds"]["args"],
            json!(["mcp", "serve", "--exposure", "commands", "--profile", "pls"])
        );
    }

    #[test]
    fn merge_keeps_other_servers_and_keys() {
        let existing = json!({ "inputs": [], "servers": { "other": { "type": "stdio", "command": "x" }, "ds": { "command": "old" } } });
        let entry = default_entry("vscode", std::path::Path::new("/new/ds"));
        let merged = merge(&existing, &entry).expect("merged");
        assert_eq!(merged["inputs"], json!([]));
        assert_eq!(merged["servers"]["other"]["command"], "x");
        assert_eq!(merged["servers"]["ds"]["command"], "/new/ds");
    }

    #[test]
    fn a_non_object_root_is_refused_not_replaced() {
        assert!(
            merge(
                &json!([1, 2]),
                &default_entry("vscode", std::path::Path::new("ds"))
            )
            .is_none()
        );
    }

    /// An isolated directory under the system temp root. Never a real host
    /// configuration path: these tests write, and a user's `mcp.json` is not
    /// a fixture.
    fn scratch(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ds-mcp-install-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn writing_creates_the_file_and_a_malformed_one_is_left_alone() {
        let dir = scratch("malformed");
        let path = dir.join("User").join("mcp.json");
        let entry = default_entry("vscode", std::path::Path::new("/usr/bin/ds"));
        merge_into_file(&path, "vscode", &entry).expect("write");
        let written: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            written["servers"]["ds"]["args"],
            json!(["mcp", "serve", "--exposure", "chapters"])
        );
        std::fs::write(&path, b"{ not json").unwrap();
        let refused = merge_into_file(&path, "vscode", &entry).unwrap_err();
        assert_eq!(refused.code(), "mcp_config_unwritable");
        assert_eq!(std::fs::read(&path).unwrap(), b"{ not json");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replacing_a_host_file_keeps_the_previous_document_as_a_backup() {
        let dir = scratch("backup");
        let path = dir.join("mcp.json");
        std::fs::create_dir_all(&dir).unwrap();
        let before = b"{\n  \"servers\": { \"other\": { \"command\": \"x\" } }\n}\n";
        std::fs::write(&path, before).unwrap();

        let entry = default_entry("vscode", std::path::Path::new("/usr/bin/ds"));
        merge_into_file(&path, "vscode", &entry).expect("write");

        let backup = super::backup_path(&path);
        assert_eq!(
            std::fs::read(&backup).unwrap(),
            before,
            "the operator's previous host configuration must be recoverable"
        );
        let written: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["servers"]["other"]["command"], "x");
        assert_eq!(written["servers"]["ds"]["command"], "/usr/bin/ds");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_successful_write_leaves_no_staging_file_behind() {
        let dir = scratch("staging");
        let path = dir.join("mcp.json");
        let entry = default_entry("vscode", std::path::Path::new("/usr/bin/ds"));
        merge_into_file(&path, "vscode", &entry).expect("write");

        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "staging files survived: {leftovers:?}"
        );

        // Every private sibling shares the target's directory, so the rename
        // can never cross a filesystem boundary and silently become a copy.
        assert_eq!(super::backup_path(&path).parent(), path.parent());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_external_edit_between_read_and_replace_is_preserved_and_refused() {
        let dir = scratch("external-edit");
        let path = dir.join("mcp.json");
        std::fs::create_dir_all(&dir).unwrap();
        let before = b"{\"servers\": {\"other\": {\"command\": \"x\"}}}";
        let external = b"{\"servers\": {\"external\": {\"command\": \"keep\"}}}";
        std::fs::write(&path, before).unwrap();
        let entry = default_entry("vscode", std::path::Path::new("/usr/bin/ds"));

        let refused = merge_into_file_with_hook(&path, "vscode", &entry, || {
            std::fs::write(&path, external).unwrap();
        })
        .unwrap_err();
        assert_eq!(refused.code(), "mcp_config_unwritable");
        assert_eq!(std::fs::read(&path).unwrap(), external);
        assert_eq!(std::fs::read(backup_path(&path)).unwrap(), before);
        assert_no_private_stages(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_installers_serialize_their_merges() {
        let dir = scratch("concurrent");
        let path = dir.join("mcp.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, b"{\"keep\": true}").unwrap();
        let start = std::sync::Arc::new(std::sync::Barrier::new(3));
        let first_path = path.clone();
        let first_start = start.clone();
        let first = std::thread::spawn(move || {
            first_start.wait();
            merge_into_file(
                &first_path,
                "vscode",
                &json!({ "servers": { "ds": { "command": "first" } } }),
            )
        });
        let second_path = path.clone();
        let second_start = start.clone();
        let second = std::thread::spawn(move || {
            second_start.wait();
            merge_into_file(
                &second_path,
                "vscode",
                &json!({ "mcpServers": { "ds": { "command": "second" } } }),
            )
        });
        start.wait();
        first.join().unwrap().expect("first installer succeeds");
        second.join().unwrap().expect("second installer succeeds");

        let written: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["keep"], true);
        assert_eq!(written["servers"]["ds"]["command"], "first");
        assert_eq!(written["mcpServers"]["ds"]["command"], "second");
        assert!(!lock_path(&path).exists(), "our lock must be cleaned up");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn assert_no_private_stages(dir: &Path) {
        let leftovers: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .filter(|name| name.contains(".ds-stage-") || name.contains(".ds-backup-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "private stages survived: {leftovers:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn hostile_target_backup_stage_and_lock_links_are_never_followed() {
        use std::os::unix::fs::symlink;

        let entry = default_entry("vscode", std::path::Path::new("/usr/bin/ds"));

        let target_dir = scratch("target-link");
        std::fs::create_dir_all(&target_dir).unwrap();
        let victim = target_dir.join("victim.json");
        std::fs::write(&victim, b"target secret").unwrap();
        let target = target_dir.join("mcp.json");
        symlink(&victim, &target).unwrap();
        assert!(merge_into_file(&target, "vscode", &entry).is_err());
        assert_eq!(std::fs::read(&victim).unwrap(), b"target secret");
        assert!(
            std::fs::symlink_metadata(&target)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let _ = std::fs::remove_dir_all(&target_dir);

        let backup_dir = scratch("backup-link");
        std::fs::create_dir_all(&backup_dir).unwrap();
        let backup_target = backup_dir.join("mcp.json");
        let before = b"{\"servers\": {\"other\": {\"command\": \"x\"}}}";
        std::fs::write(&backup_target, before).unwrap();
        let backup_victim = backup_dir.join("backup-victim.json");
        std::fs::write(&backup_victim, b"backup secret").unwrap();
        symlink(&backup_victim, backup_path(&backup_target)).unwrap();
        assert!(merge_into_file(&backup_target, "vscode", &entry).is_err());
        assert_eq!(std::fs::read(&backup_target).unwrap(), before);
        assert_eq!(std::fs::read(&backup_victim).unwrap(), b"backup secret");
        assert_no_private_stages(&backup_dir);
        let _ = std::fs::remove_dir_all(&backup_dir);

        let lock_dir = scratch("lock-link");
        std::fs::create_dir_all(&lock_dir).unwrap();
        let lock_target = lock_dir.join("mcp.json");
        let lock_victim = lock_dir.join("lock-victim");
        std::fs::write(&lock_victim, b"lock secret").unwrap();
        symlink(&lock_victim, lock_path(&lock_target)).unwrap();
        assert!(merge_into_file(&lock_target, "vscode", &entry).is_err());
        assert_eq!(std::fs::read(&lock_victim).unwrap(), b"lock secret");
        assert!(
            std::fs::symlink_metadata(lock_path(&lock_target))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let _ = std::fs::remove_dir_all(&lock_dir);

        // A pre-created stage is rejected by exclusive creation; it is neither
        // followed nor truncated, even though a hostile name resembles ours.
        let stage_dir = scratch("stage-link");
        std::fs::create_dir_all(&stage_dir).unwrap();
        let stage_victim = stage_dir.join("stage-victim");
        std::fs::write(&stage_victim, b"stage secret").unwrap();
        let hostile_stage = stage_dir.join(".mcp.json.ds-stage-attacker.tmp");
        symlink(&stage_victim, &hostile_stage).unwrap();
        assert!(create_exclusive(&hostile_stage).is_err());
        assert_eq!(std::fs::read(&stage_victim).unwrap(), b"stage secret");
        let hostile_regular_stage = stage_dir.join(".mcp.json.ds-stage-foreign.tmp");
        std::fs::write(&hostile_regular_stage, b"foreign stage").unwrap();
        assert!(create_exclusive(&hostile_regular_stage).is_err());
        assert_eq!(
            std::fs::read(&hostile_regular_stage).unwrap(),
            b"foreign stage"
        );
        let _ = std::fs::remove_dir_all(&stage_dir);
    }

    #[test]
    fn a_precreated_regular_lock_is_not_overwritten_or_removed() {
        let dir = scratch("foreign-lock");
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("mcp.json");
        let lock = lock_path(&target);
        std::fs::write(&lock, b"foreign lock").unwrap();
        assert!(InstallLock::acquire_with_attempts(&target, 1).is_err());
        assert_eq!(std::fs::read(&lock).unwrap(), b"foreign lock");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn target_and_backup_keep_restrictive_modes() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = scratch("modes");
        let path = dir.join("mcp.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, b"{\"servers\": {}}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let entry = default_entry("vscode", std::path::Path::new("/usr/bin/ds"));
        merge_into_file(&path, "vscode", &entry).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(backup_path(&path))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_declared_effect_makes_the_documented_yes_real() {
        // F7: this command writes a *user-level host* file, not a workspace
        // file. `local_file_write` is not in the confirmation set, so the
        // `--write --yes` its own help and `docs/reference/mcp.md` ask for was
        // decorative. Reverting either half of this fails here.
        assert_eq!(COMMAND.effect, Effect::MachineWrite);
        assert!(COMMAND.effect.needs_confirmation());
        assert!(
            COMMAND
                .refusals
                .iter()
                .any(|refusal| refusal.code == "confirmation_required"),
            "a gate a caller cannot discover from help is not a contract"
        );
        assert!(
            COMMAND
                .refusals
                .iter()
                .any(|refusal| refusal.code == "mcp_capabilities_unavailable"),
            "`build_identity` can emit this on the way to writing the entry"
        );
    }
}
