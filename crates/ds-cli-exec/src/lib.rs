//! Calling another DS executable.
//!
//! `ds` reaches a domain owner one of two ways. Where the owner is a pure
//! library with a clean boundary, `ds` links it. Where the owning workspace
//! deliberately chose *process* separation and wrote down why, `ds` calls the
//! binary — and this crate is that call.
//!
//! `ds-report`'s own header states the rule those owners settled on:
//!
//! > one named subcommand per call — never a caller-supplied argv … a typed
//! > request file — not flags built from model output … a machine-readable
//! > result document — never parsed stdout prose.
//!
//! So there is deliberately **no** `run(name, argv)` here that a command
//! could hand a caller-supplied vector to. A domain declares a
//! [`External`] once, names the subcommand itself, and passes typed
//! arguments it constructed. A subcommand no `ds` command names is not
//! reachable from `ds`, which is the property the rule exists to preserve.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::Availability;
use serde_json::json;

/// Cap on what a called binary may return. Beyond this the output is
/// truncated and the invocation refused: a result larger than this is not a
/// result `ds` should be inlining anyway — it belongs in a file the callee
/// already wrote.
pub const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
/// Cap on an in-memory typed request transferred to a sibling through stdin.
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;

/// How often the wait loop checks. Small enough that a fast task is not
/// delayed, large enough not to spin.
const POLL: Duration = Duration::from_millis(20);

/// One sibling executable `ds` is allowed to call.
pub struct External {
    /// The executable's name, without any platform suffix. This is also the
    /// name Tauri installs the sidecar under, so the deployed lookup is a
    /// sibling of `ds` itself.
    pub name: &'static str,
    /// An absolute-path override, for a developer running an unpackaged
    /// build. Read from the environment because there is no sensible flag
    /// for it on every command that might call this binary.
    pub env_override: &'static str,
    /// The owning repository, named in refusals so "install it" is
    /// actionable rather than a shrug.
    pub owner: &'static str,
    /// What to do when it is missing.
    pub remedy: &'static str,
    /// The stable code used when it cannot be found. Each domain owns its
    /// own, so a caller branching on `error.code` learns *which* engine is
    /// absent, not merely that one is.
    pub missing_code: &'static str,
}

/// A completed call.
pub struct Completed {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// True when either stream hit [`MAX_OUTPUT_BYTES`].
    pub truncated: bool,
}

impl Completed {
    pub fn succeeded(&self) -> bool {
        self.status == Some(0)
    }
}

impl External {
    /// Where the executable is, or nothing.
    ///
    /// Order, and the reason for it:
    ///
    /// 1. **The environment override.** Explicit beats inferred, always.
    /// 2. **A sibling of the running `ds`.** This is the deployed case: the
    ///    desktop installs every sidecar next to the shell's executable, so
    ///    an installed `ds` finds an installed `ds-report` with no
    ///    configuration and no `PATH` dependency.
    /// 3. **`PATH`.** For a developer who put one there.
    ///
    /// `PATH` is deliberately last. If it were first, a stale binary earlier
    /// in someone's `PATH` would silently outrank the one shipped alongside
    /// the application — and the resulting wrong answer would look like a
    /// correct one.
    pub fn locate(&self) -> Option<PathBuf> {
        if let Some(raw) = std::env::var_os(self.env_override) {
            let path = PathBuf::from(raw);
            // An override that does not resolve is an operator error worth
            // surfacing, not a reason to quietly fall through to a different
            // binary than the one they named.
            return path.is_file().then_some(path);
        }

        if let Some(sibling) = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join(self.file_name())))
            .filter(|path| path.is_file())
        {
            return Some(sibling);
        }

        self.on_path()
    }

    /// Availability, resolved with filesystem metadata only.
    ///
    /// This must never execute the binary. Domain help and `ds doctor` both
    /// call it, and a discovery call that spawns processes is a discovery
    /// call nobody can afford to make.
    pub fn availability(&self) -> Availability {
        match self.locate() {
            Some(_) => Availability::Available,
            None => Availability::unavailable(
                self.missing_code,
                format!("`{}` was not found", self.name),
                self.remedy,
            ),
        }
    }

    /// Invoke one named subcommand with typed arguments.
    ///
    /// `subcommand` is a `&'static str` on purpose: it comes from a `ds`
    /// command's own source, never from a caller. `args` carries the typed
    /// values that command constructed.
    pub fn call(
        &self,
        subcommand: &'static str,
        args: &[OsString],
        timeout: Duration,
    ) -> Result<Completed, Failure> {
        let executable = self.locate().ok_or_else(|| {
            Failure::unavailable(self.missing_code, format!("`{}` was not found", self.name))
            .remedy(self.remedy)
            .detail(
                json!({ "binary": self.name, "owner": self.owner, "override": self.env_override }),
            )
        })?;

        let mut command = Command::new(&executable);
        command
            .arg(subcommand)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|error| {
            Failure::unavailable(
                self.missing_code,
                format!("`{}` could not be started", self.name),
            )
            .remedy(self.remedy)
            .detail(json!({ "detail": error.kind().to_string() }))
        })?;

        // Drain both pipes on their own threads. A callee that fills a pipe
        // while we are blocked waiting on the other one deadlocks, and
        // `task-schemas` is already tens of kilobytes.
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdout_reader = std::thread::spawn(move || read_bounded(stdout));
        let stderr_reader = std::thread::spawn(move || read_bounded(stderr));

        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        break None;
                    }
                    std::thread::sleep(POLL);
                }
                Err(error) => {
                    let _ = child.kill();
                    return Err(Failure::internal(
                        "callee_wait_failed",
                        format!("could not wait for `{}`", self.name),
                    )
                    .detail(json!({ "detail": error.kind().to_string() })));
                }
            }
        };

        let (stdout, stdout_truncated) = stdout_reader.join().unwrap_or_default();
        let (stderr, stderr_truncated) = stderr_reader.join().unwrap_or_default();

        let Some(status) = status else {
            return Err(Failure::failed(
                "callee_timed_out",
                format!(
                    "`{}` did not finish within {}s",
                    self.name,
                    timeout.as_secs()
                ),
            )
            .remedy("retry with a smaller input, or investigate the engine")
            .detail(json!({ "binary": self.name, "timeout_s": timeout.as_secs() })));
        };

        Ok(Completed {
            status: status.code(),
            stdout,
            stderr,
            truncated: stdout_truncated || stderr_truncated,
        })
    }

    /// Invoke one named subcommand with one bounded typed stdin document.
    ///
    /// This is the credential-adjacent handoff for owner contracts containing
    /// short-lived transport authority: the bytes are never placed in argv or
    /// a temporary file. The subcommand remains static and arguments remain a
    /// command-owned typed projection, exactly as in [`Self::call`].
    pub fn call_with_stdin(
        &self,
        subcommand: &'static str,
        args: &[OsString],
        input: &[u8],
        timeout: Duration,
    ) -> Result<Completed, Failure> {
        if input.is_empty() || input.len() > MAX_INPUT_BYTES {
            return Err(Failure::invalid(
                "callee_input_bounded",
                format!(
                    "the typed `{}` stdin document is empty or exceeds {} bytes",
                    self.name, MAX_INPUT_BYTES
                ),
            )
            .remedy("use the owner-supported bounded input contract"));
        }
        let executable = self.locate().ok_or_else(|| {
            Failure::unavailable(self.missing_code, format!("`{}` was not found", self.name))
                .remedy(self.remedy)
                .detail(
                    json!({ "binary": self.name, "owner": self.owner, "override": self.env_override }),
                )
        })?;

        let mut command = Command::new(&executable);
        command
            .arg(subcommand)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| {
            Failure::unavailable(
                self.missing_code,
                format!("`{}` could not be started", self.name),
            )
            .remedy(self.remedy)
            .detail(json!({ "detail": error.kind().to_string() }))
        })?;

        let mut stdin = child.stdin.take().ok_or_else(|| {
            Failure::internal(
                "callee_stdin_failed",
                format!("could not open `{}` typed stdin", self.name),
            )
        })?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdout_reader = std::thread::spawn(move || read_bounded(stdout));
        let stderr_reader = std::thread::spawn(move || read_bounded(stderr));
        let deadline = Instant::now() + timeout;
        let (wait_result, write_succeeded) = std::thread::scope(|scope| {
            let writer = scope.spawn(|| {
                let result = stdin.write_all(input);
                drop(stdin);
                result
            });
            let wait_result = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Ok(Some(status)),
                    Ok(None) => {
                        if Instant::now() >= deadline {
                            let _ = child.kill();
                            let _ = child.wait();
                            break Ok(None);
                        }
                        std::thread::sleep(POLL);
                    }
                    Err(error) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break Err(error);
                    }
                }
            };
            let write_succeeded = writer.join().is_ok_and(|result| result.is_ok());
            (wait_result, write_succeeded)
        });

        let (stdout, stdout_truncated) = stdout_reader.join().unwrap_or_default();
        let (stderr, stderr_truncated) = stderr_reader.join().unwrap_or_default();
        let status = match wait_result {
            Err(error) => {
                return Err(Failure::internal(
                    "callee_wait_failed",
                    format!("could not wait for `{}`", self.name),
                )
                .detail(json!({ "detail": error.kind().to_string() })));
            }
            Ok(Some(status)) => status,
            Ok(None) => {
                return Err(Failure::failed(
                    "callee_timed_out",
                    format!(
                        "`{}` did not finish within {}s",
                        self.name,
                        timeout.as_secs()
                    ),
                )
                .remedy("retry with a smaller input, or investigate the engine"));
            }
        };
        if status.success() && !write_succeeded {
            return Err(Failure::failed(
                "callee_stdin_failed",
                format!("`{}` did not consume its complete typed input", self.name),
            )
            .remedy("update ds and the owner engine to one matching release"));
        }
        Ok(Completed {
            status: status.code(),
            stdout,
            stderr,
            truncated: stdout_truncated || stderr_truncated,
        })
    }

    /// Parse a callee's stdout as JSON, or refuse with a typed contract
    /// mismatch. A discovery command that returned prose where a document was
    /// promised is a contract break, not a parse inconvenience.
    pub fn call_json(
        &self,
        subcommand: &'static str,
        args: &[OsString],
        timeout: Duration,
    ) -> Result<serde_json::Value, Failure> {
        let completed = self.call(subcommand, args, timeout)?;
        if !completed.succeeded() {
            return Err(self.failure_from(&completed, subcommand));
        }
        serde_json::from_str(&completed.stdout).map_err(|error| {
            Failure::failed(
                "callee_contract_mismatch",
                format!(
                    "`{} {subcommand}` did not return a JSON document",
                    self.name
                ),
            )
            .remedy("update `ds` and the engine to matching releases")
            .detail(json!({ "detail": error.to_string() }))
        })
    }

    /// Map a non-zero exit into a typed refusal.
    ///
    /// The callee's stderr is bounded and carried in `detail` rather than
    /// becoming the message: it is the engine's words, useful for a human,
    /// and not something a caller should be matching on. `error.code` is the
    /// stable thing.
    pub fn failure_from(&self, completed: &Completed, subcommand: &str) -> Failure {
        let mut failure = Failure::failed(
            "engine_refused",
            format!("`{} {subcommand}` did not complete", self.name),
        )
        .remedy("read `detail.engine` for the engine's own message")
        .detail(json!({
            "binary": self.name,
            "subcommand": subcommand,
            "exit_code": completed.status,
            "engine": bounded_message(&completed.stderr, &completed.stdout),
        }));
        if completed.truncated {
            failure = failure.remedy("the engine's output exceeded its bound and was truncated");
        }
        failure
    }

    fn file_name(&self) -> String {
        if cfg!(windows) {
            format!("{}.exe", self.name)
        } else {
            self.name.to_string()
        }
    }

    fn on_path(&self) -> Option<PathBuf> {
        let file_name = self.file_name();
        std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(&file_name))
                .find(|candidate| is_executable(candidate))
        })
    }
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Read a stream to the byte bound, reporting whether it was cut short.
fn read_bounded(stream: Option<impl Read>) -> (String, bool) {
    let Some(mut stream) = stream else {
        return (String::new(), false);
    };
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let mut truncated = false;
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                let room = MAX_OUTPUT_BYTES.saturating_sub(buffer.len());
                if room == 0 {
                    truncated = true;
                    // Keep draining so the callee is never blocked on a full
                    // pipe; we simply stop keeping the bytes.
                    continue;
                }
                buffer.extend_from_slice(&chunk[..read.min(room)]);
                if read > room {
                    truncated = true;
                }
            }
            Err(_) => break,
        }
    }
    (String::from_utf8_lossy(&buffer).into_owned(), truncated)
}

/// The engine's own words, bounded, preferring stderr and falling back to
/// stdout. Kept to a few lines: a caller needs the gist, and the full
/// document — where the engine wrote one — is already on disk.
fn bounded_message(stderr: &str, stdout: &str) -> String {
    let source = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(6)
        .map(|line| line.chars().take(200).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use super::*;

    #[test]
    fn typed_stdin_is_bounded_delivered_and_timeout_safe() {
        let root = std::env::temp_dir().join(format!("ds-cli-exec-stdin-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("fixture-owner");
        fs::write(
            &executable,
            b"#!/bin/sh\ncase \"$1\" in\n  transfer) wc -c ;;\n  stall) sleep 5 ;;\n  *) exit 9 ;;\nesac\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&executable, permissions).unwrap();

        const OVERRIDE: &str = "DS_CLI_EXEC_TEST_STDIN_OWNER";
        // This one test is the only user of its unique environment key.
        unsafe { std::env::set_var(OVERRIDE, &executable) };
        let owner = External {
            name: "fixture-owner",
            env_override: OVERRIDE,
            owner: "test",
            remedy: "repair fixture",
            missing_code: "fixture_missing",
        };

        let input = vec![b'x'; 256 * 1024];
        let completed = owner
            .call_with_stdin("transfer", &[], &input, Duration::from_secs(2))
            .unwrap();
        assert!(completed.succeeded());
        assert_eq!(completed.stdout.trim(), input.len().to_string());

        let oversized = vec![b'x'; MAX_INPUT_BYTES + 1];
        assert_eq!(
            owner
                .call_with_stdin("transfer", &[], &oversized, Duration::from_secs(2))
                .err()
                .expect("oversized stdin must be refused")
                .code(),
            "callee_input_bounded"
        );

        // Fill more than a typical pipe while the child never reads. The
        // timeout must kill the child, unblock the writer, and return rather
        // than deadlocking on either pipe or stdin thread.
        let blocking = vec![b'x'; 1024 * 1024];
        assert_eq!(
            owner
                .call_with_stdin("stall", &[], &blocking, Duration::from_millis(80))
                .err()
                .expect("stalled owner must time out")
                .code(),
            "callee_timed_out"
        );

        unsafe { std::env::remove_var(OVERRIDE) };
        fs::remove_file(executable).unwrap();
        fs::remove_dir(root).unwrap();
    }
}

mod solar_worker;
pub use solar_worker::start_solar_project_sync;
mod design_worker;
pub use design_worker::start_design_project_process;
