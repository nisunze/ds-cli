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

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::Availability;
use serde_json::json;

pub mod addons;
mod managed_modules;
mod process_tree;
mod session;
pub use session::NetworkProfileProcess;

/// Cap on what a called binary may return. Beyond this the output is
/// truncated and the invocation refused: a result larger than this is not a
/// result `ds` should be inlining anyway — it belongs in a file the callee
/// already wrote.
pub const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
/// Cap on an in-memory typed request transferred to a sibling through stdin.
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
type StderrObserver<'a> = &'a dyn Fn(&[u8]);

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

/// One resolved engine for an accepted run. Resolving a symlink once keeps
/// subsequent calls on the same version when an updater switches its active
/// pointer. The version directory must remain immutable and retained until
/// all runs using it have finished.
pub struct CapturedExternal {
    owner: &'static External,
    executable: PathBuf,
    managed_identity: Option<managed_modules::ManagedIdentity>,
}

impl CapturedExternal {
    fn verify_managed(&self) -> Result<(), Failure> {
        if let Some(identity) = &self.managed_identity {
            managed_modules::verify(&self.executable, identity)?;
        }
        Ok(())
    }
    fn verified<T>(&self, call: impl FnOnce() -> Result<T, Failure>) -> Result<T, Failure> {
        self.verify_managed()?;
        let result = call()?;
        self.verify_managed()?;
        Ok(result)
    }
    /// Canonical executable captured for this accepted run. Native hosts may
    /// reuse their bounded process collectors without resolving an active module
    /// pointer again or requiring the module in their application bundle.
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn call(
        &self,
        subcommand: &'static str,
        args: &[OsString],
        timeout: Duration,
    ) -> Result<Completed, Failure> {
        self.verified(|| {
            self.owner
                .call_at(&self.executable, subcommand, args, timeout)
        })
    }

    /// Invoke one fixed Network task under its host CPU and cancellation budget.
    /// The control pipe accepts only the closed Network cancel frame; other
    /// owner protocols continue to use their existing call/typed-input methods.
    pub fn call_cancelled(
        &self,
        subcommand: &'static str,
        args: &[OsString],
        timeout: Duration,
        workers: usize,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Completed, Failure> {
        if workers == 0 {
            return Err(Failure::invalid(
                "callee_worker_budget_invalid",
                "the native worker budget must be positive",
            ));
        }
        self.verified(|| {
            self.owner.call_at_supervised(
                &self.executable,
                subcommand,
                args,
                timeout,
                Some(workers),
                cancel,
                None,
            )
        })
    }

    /// Observe bounded stderr chunks on the calling thread while the owned
    /// process runs. The observer must return promptly; stdout, cancellation,
    /// deadlines and process-tree cleanup keep their existing bounds.
    pub fn call_cancelled_observed(
        &self,
        subcommand: &'static str,
        args: &[OsString],
        timeout: Duration,
        workers: usize,
        cancel: &dyn Fn() -> bool,
        stderr_observer: &dyn Fn(&[u8]),
    ) -> Result<Completed, Failure> {
        if workers == 0 {
            return Err(Failure::invalid(
                "callee_worker_budget_invalid",
                "the native worker budget must be positive",
            ));
        }
        self.verified(|| {
            self.owner.call_at_supervised(
                &self.executable,
                subcommand,
                args,
                timeout,
                Some(workers),
                cancel,
                Some(stderr_observer),
            )
        })
    }

    /// Observe one declared host command without replacing its environment or
    /// inventing an engine control protocol. Stdin is closed. Cancellation
    /// terminates and reaps the same owned process tree before returning.
    /// Native adapters must keep their physical lease until this call returns.
    pub fn call_host_observed(
        &self,
        subcommand: &'static str,
        args: &[OsString],
        timeout: Duration,
        cancel: &dyn Fn() -> bool,
        stderr_observer: &dyn Fn(&[u8]),
    ) -> Result<Completed, Failure> {
        self.verified(|| {
            self.owner.call_at_supervised(
                &self.executable,
                subcommand,
                args,
                timeout,
                None,
                cancel,
                Some(stderr_observer),
            )
        })
    }

    pub fn call_json(
        &self,
        subcommand: &'static str,
        args: &[OsString],
        timeout: Duration,
    ) -> Result<serde_json::Value, Failure> {
        let completed = self.call(subcommand, args, timeout)?;
        if !completed.succeeded() {
            return Err(self.owner.failure_from(&completed, subcommand));
        }
        if completed.truncated {
            return Err(Failure::failed(
                "callee_contract_mismatch",
                "the captured engine's JSON exceeded its output bound",
            )
            .remedy("use a bounded file result through the engine's typed task"));
        }
        serde_json::from_str(&completed.stdout).map_err(|error| {
            Failure::failed(
                "callee_contract_mismatch",
                "the captured engine returned invalid JSON",
            )
            .detail(json!({"detail": error.to_string()}))
        })
    }
}

impl Completed {
    pub fn succeeded(&self) -> bool {
        self.status == Some(0)
    }
}

impl External {
    /// Resolve the exact captured native lane's installed addon. A malformed
    /// managed selection refuses; it never borrows another lane or PATH.
    pub fn capture_for_lane(&'static self, lane: &str) -> Result<CapturedExternal, Failure> {
        self.capture_for_lane_in(lane, &managed_modules::module_base()?)
    }
    /// Privileged native host support for an explicitly relocated owned store.
    /// This path is not an IPC/CLI artifact-path argument.
    pub fn capture_for_lane_in(
        &'static self,
        lane: &str,
        base: &Path,
    ) -> Result<CapturedExternal, Failure> {
        if !matches!(lane, "stable" | "canary") {
            return Err(Failure::invalid(
                "module_lane_invalid",
                "capture exactly stable or canary",
            ));
        }
        if std::env::var_os(self.env_override).is_some() {
            return self.capture();
        }
        self.capture_managed_for_lane_in(lane, base)?
            .map_or_else(|| self.capture(), Ok)
    }

    /// Resolve only a retained native managed selection. `None` lets an owning
    /// host preserve its established unmanaged policy (for example Solar's
    /// explicit override or packaged sibling only). No PATH or child probe.
    /// Hosts must apply their explicit development override before this lookup.
    pub fn capture_managed_for_lane(
        &'static self,
        lane: &str,
    ) -> Result<Option<CapturedExternal>, Failure> {
        self.capture_managed_for_lane_in(lane, &managed_modules::module_base()?)
    }

    /// Privileged native host support for an explicitly relocated owned store.
    pub fn capture_managed_for_lane_in(
        &'static self,
        lane: &str,
        base: &Path,
    ) -> Result<Option<CapturedExternal>, Failure> {
        match managed_modules::selected(self, base, lane)? {
            Some((path, identity)) => {
                let mut captured = self.capture_pinned(&path)?;
                captured.managed_identity = Some(identity);
                Ok(Some(captured))
            }
            None => Ok(None),
        }
    }
    /// Capture by filesystem metadata only; discovery starts no process.
    pub fn capture(&'static self) -> Result<CapturedExternal, Failure> {
        self.capture_path(self.locate())
    }

    /// Reopen an already captured native executable for a retained run.
    /// This never consults the active pointer, environment override or PATH.
    /// The application owner must verify the retained package digest/identity
    /// before executing; an explicit path is not an installation trust receipt.
    pub fn capture_pinned(&'static self, path: &Path) -> Result<CapturedExternal, Failure> {
        if !path.is_absolute()
            || !std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
        {
            return Err(Failure::unavailable(
                self.missing_code,
                format!("the pinned `{}` executable was not found", self.name),
            )
            .remedy(self.remedy));
        }
        self.capture_path(Some(path.to_path_buf()))
    }

    fn capture_path(&'static self, path: Option<PathBuf>) -> Result<CapturedExternal, Failure> {
        let executable = path
            .and_then(|path| std::fs::canonicalize(path).ok())
            .filter(|path| is_executable(path))
            .ok_or_else(|| {
                Failure::unavailable(self.missing_code, format!("`{}` was not found", self.name))
                    .remedy(self.remedy)
            })?;
        Ok(CapturedExternal {
            owner: self,
            executable,
            managed_identity: None,
        })
    }
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
        let executable = std::env::current_exe().ok();
        let path = std::env::var_os("PATH");
        self.locate_for_paths(executable.as_deref(), path.as_deref())
    }

    fn locate_for_paths(&self, executable: Option<&Path>, path: Option<&OsStr>) -> Option<PathBuf> {
        if let Some(raw) = std::env::var_os(self.env_override) {
            let path = PathBuf::from(raw);
            // An override that does not resolve is an operator error worth
            // surfacing, not a reason to quietly fall through to a different
            // binary than the one they named.
            return path.is_file().then_some(path);
        }

        if let Some(executable) = executable
            && let Some((sibling, sealed_lane)) = packaged_sibling(executable, &self.file_name())
        {
            if sibling.is_file() {
                return Some(sibling);
            }
            // A packaged Canary CLI must not quietly borrow a Stable sidecar
            // from PATH. Its package owns the lane-suffixed siblings.
            if sealed_lane {
                return None;
            }
        }

        self.on_path_in(path)
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

        self.call_at(&executable, subcommand, args, timeout)
    }

    fn call_at(
        &self,
        executable: &Path,
        subcommand: &'static str,
        args: &[OsString],
        timeout: Duration,
    ) -> Result<Completed, Failure> {
        self.call_at_supervised(executable, subcommand, args, timeout, None, &|| false, None)
    }
    #[allow(clippy::too_many_arguments)]
    fn call_at_supervised(
        &self,
        executable: &Path,
        subcommand: &'static str,
        args: &[OsString],
        timeout: Duration,
        workers: Option<usize>,
        cancel: &dyn Fn() -> bool,
        stderr_observer: Option<StderrObserver<'_>>,
    ) -> Result<Completed, Failure> {
        if cancel() {
            return Err(Failure::failed(
                "callee_cancelled",
                format!("`{}` was cancelled before launch", self.name),
            ));
        }
        let mut command = Command::new(executable);
        command
            .arg(subcommand)
            .args(args)
            .stdin(if workers.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(workers) = workers {
            command.env_clear();
            for key in [
                "SystemRoot",
                "WINDIR",
                "COMSPEC",
                "TEMP",
                "TMP",
                "TMPDIR",
                "LANG",
                "LC_ALL",
                "TZ",
            ] {
                if let Some(value) = std::env::var_os(key) {
                    command.env(key, value);
                }
            }
            let workers = workers
                .min(std::thread::available_parallelism().map_or(1, usize::from))
                .max(1)
                .to_string();
            for key in [
                "DS_NETWORK_WORKER_THREADS",
                "RAYON_NUM_THREADS",
                "OMP_NUM_THREADS",
                "OPENBLAS_NUM_THREADS",
                "MKL_NUM_THREADS",
                "NUMEXPR_NUM_THREADS",
                "VECLIB_MAXIMUM_THREADS",
            ] {
                command.env(key, &workers);
            }
        }
        process_tree::Tree::prepare(&mut command);
        let mut child = command.spawn().map_err(|error| {
            Failure::unavailable(
                self.missing_code,
                format!("`{}` could not be started", self.name),
            )
            .remedy(self.remedy)
            .detail(json!({"detail":error.kind().to_string()}))
        })?;
        let mut tree = match process_tree::Tree::attach(&mut child) {
            Ok(tree) => tree,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Failure::internal(
                    "callee_supervision_failed",
                    "could not establish native process-tree ownership",
                )
                .detail(json!({"detail":error.kind().to_string()})));
            }
        };
        let mut control = child.stdin.take();
        let mut cancellation_started = None::<Instant>;
        let overflow = Arc::new(AtomicBool::new(false));
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdout_overflow = Arc::clone(&overflow);
        let stderr_overflow = Arc::clone(&overflow);
        let (stderr_events, stderr_receiver) = std::sync::mpsc::channel();
        let stderr_events = stderr_observer.map(|_| stderr_events);
        let stdout_reader =
            std::thread::spawn(move || read_bounded_signal(stdout, &stdout_overflow));
        let stderr_reader = std::thread::spawn(move || {
            read_bounded_signal_observed(stderr, &stderr_overflow, stderr_events)
        });
        enum Stop {
            Done,
            Cancelled,
            Deadline,
            Overflow,
            Wait(std::io::Error),
        }
        let started = Instant::now();
        let stop = loop {
            if let Some(observer) = stderr_observer {
                for chunk in stderr_receiver.try_iter() {
                    observer(&chunk);
                }
            }
            match tree.try_wait(&mut child) {
                Ok(Some(_)) => break Stop::Done,
                Ok(None) => {}
                Err(error) => break Stop::Wait(error),
            }
            if cancel() && cancellation_started.is_none() {
                if let Some(pipe) = control.as_mut() {
                    let _ = pipe.write_all(
                        b"{\"schema\":\"ds.network.control/v1\",\"operation\":\"cancel\"}\n",
                    );
                    let _ = pipe.flush();
                }
                cancellation_started = Some(Instant::now());
            }
            if cancellation_started
                .is_some_and(|started| started.elapsed() >= Duration::from_millis(300))
            {
                break Stop::Cancelled;
            }
            if overflow.load(Ordering::Acquire) {
                break Stop::Overflow;
            }
            if started.elapsed() >= timeout {
                break Stop::Deadline;
            }
            std::thread::sleep(POLL);
        };
        // Descendants must close their pipes before either drain can be joined.
        tree.terminate();
        drop(control);
        if !matches!(stop, Stop::Done) {
            let _ = child.kill();
        }
        let waited = child.wait();
        let (stdout, stdout_truncated) = stdout_reader.join().unwrap_or_default();
        let (stderr, stderr_truncated) = stderr_reader.join().unwrap_or_default();
        if let Some(observer) = stderr_observer {
            for chunk in stderr_receiver.try_iter() {
                observer(&chunk);
            }
        }
        match stop {
            Stop::Cancelled => {
                return Err(Failure::failed(
                    "callee_cancelled",
                    format!("`{}` was cancelled", self.name),
                ));
            }
            Stop::Deadline => {
                return Err(Failure::failed(
                    "callee_timed_out",
                    format!(
                        "`{}` did not finish within {}s",
                        self.name,
                        timeout.as_secs()
                    ),
                )
                .remedy("retry with a smaller input, or investigate the engine")
                .detail(json!({"binary":self.name,"timeout_s":timeout.as_secs()})));
            }
            Stop::Wait(error) => {
                return Err(Failure::internal(
                    "callee_wait_failed",
                    format!("could not wait for `{}`", self.name),
                )
                .detail(json!({"detail":error.kind().to_string()})));
            }
            _ => {}
        }
        let status = waited.map_err(|error| {
            Failure::internal(
                "callee_wait_failed",
                format!("could not reap `{}`", self.name),
            )
            .detail(json!({"detail":error.kind().to_string()}))
        })?;
        Ok(Completed {
            status: status.code(),
            stdout,
            stderr,
            truncated: stdout_truncated || stderr_truncated || overflow.load(Ordering::Acquire),
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
        process_tree::Tree::prepare(&mut command);
        let mut child = command.spawn().map_err(|error| {
            Failure::unavailable(
                self.missing_code,
                format!("`{}` could not be started", self.name),
            )
            .remedy(self.remedy)
            .detail(json!({ "detail": error.kind().to_string() }))
        })?;
        let mut tree = match process_tree::Tree::attach(&mut child) {
            Ok(tree) => tree,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Failure::internal(
                    "callee_supervision_failed",
                    "could not establish native process-tree ownership",
                )
                .detail(json!({"detail":error.kind().to_string()})));
            }
        };

        let Some(mut stdin) = child.stdin.take() else {
            tree.terminate();
            let _ = child.kill();
            let _ = child.wait();
            return Err(Failure::internal(
                "callee_stdin_failed",
                format!("could not open `{}` typed stdin", self.name),
            ));
        };
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let overflow = Arc::new(AtomicBool::new(false));
        let stdout_overflow = Arc::clone(&overflow);
        let stderr_overflow = Arc::clone(&overflow);
        let stdout_reader =
            std::thread::spawn(move || read_bounded_signal(stdout, &stdout_overflow));
        let stderr_reader =
            std::thread::spawn(move || read_bounded_signal(stderr, &stderr_overflow));
        let deadline = Instant::now() + timeout;
        let (wait_result, write_succeeded) = std::thread::scope(|scope| {
            let writer = scope.spawn(|| {
                let result = stdin.write_all(input);
                drop(stdin);
                result
            });
            let wait_result = loop {
                match tree.try_wait(&mut child) {
                    Ok(Some(status)) => break Ok(Some(status)),
                    Ok(None) => {
                        if overflow.load(Ordering::Acquire) || Instant::now() >= deadline {
                            break Ok(None);
                        }
                        std::thread::sleep(POLL);
                    }
                    Err(error) => {
                        break Err(error);
                    }
                }
            };
            // A descendant may hold stdin or either output pipe even after
            // the leader exits. Stop that group before joining any I/O thread.
            tree.terminate();
            if !matches!(wait_result, Ok(Some(_))) {
                let _ = child.kill();
            }
            let reaped = child.wait();
            let write_succeeded = writer.join().is_ok_and(|result| result.is_ok());
            (
                wait_result.and_then(|status| reaped.map(|_| status)),
                write_succeeded,
            )
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
            Ok(Some(status)) => Some(status),
            Ok(None) if overflow.load(Ordering::Acquire) => None,
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
        if status.is_some_and(|status| status.success()) && !write_succeeded {
            return Err(Failure::failed(
                "callee_stdin_failed",
                format!("`{}` did not consume its complete typed input", self.name),
            )
            .remedy("update ds and the owner engine to one matching release"));
        }
        Ok(Completed {
            status: status.and_then(|status| status.code()),
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
        if completed.truncated {
            return Err(Failure::failed(
                "callee_contract_mismatch",
                "the engine's JSON exceeded its output bound",
            )
            .remedy("use a bounded file result through the engine's typed task"));
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

    fn on_path_in(&self, path: Option<&OsStr>) -> Option<PathBuf> {
        let file_name = self.file_name();
        path.and_then(|path| {
            std::env::split_paths(path)
                .map(|dir| dir.join(&file_name))
                .find(|candidate| is_executable(candidate))
        })
    }
}

/// Resolve a packaged sidecar from the executable that launched this process.
///
/// The Canary Debian package installs `ds-canary` with lane-suffixed sidecars
/// alongside it. That package is intentionally co-installable with Stable, so
/// a missing Canary sidecar is not allowed to fall through to Stable on PATH.
fn packaged_sibling(executable: &Path, sidecar: &str) -> Option<(PathBuf, bool)> {
    let sealed_canary = cfg!(not(windows))
        && executable
            .file_name()
            .is_some_and(|name| name == "ds-canary");
    let sidecar = if sealed_canary {
        format!("{sidecar}-canary")
    } else {
        sidecar.to_string()
    };
    executable
        .parent()
        .map(|directory| (directory.join(sidecar), sealed_canary))
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

/// Read a stream to the byte bound, reporting overflow or invalid UTF-8.
fn read_bounded_signal(stream: Option<impl Read>, overflow: &AtomicBool) -> (String, bool) {
    read_bounded_signal_observed(stream, overflow, None)
}

fn read_bounded_signal_observed(
    stream: Option<impl Read>,
    overflow: &AtomicBool,
    events: Option<std::sync::mpsc::Sender<Vec<u8>>>,
) -> (String, bool) {
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
                    overflow.store(true, Ordering::Release);
                    // Keep draining so the callee is never blocked on a full
                    // pipe; we simply stop keeping the bytes.
                    continue;
                }
                buffer.extend_from_slice(&chunk[..read.min(room)]);
                if let Some(events) = &events {
                    // Only retained bytes enter this queue, so total queued
                    // data can never exceed MAX_OUTPUT_BYTES.
                    let _ = events.send(chunk[..read.min(room)].to_vec());
                }
                if read > room {
                    truncated = true;
                    overflow.store(true, Ordering::Release);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => {
                truncated = true;
                overflow.store(true, Ordering::Release);
                break;
            }
        }
    }
    let decoded = String::from_utf8_lossy(&buffer);
    if matches!(decoded, std::borrow::Cow::Owned(_)) {
        truncated = true;
        overflow.store(true, Ordering::Release);
    }
    let mut text = decoded.into_owned();
    if text.len() > MAX_OUTPUT_BYTES {
        let mut end = MAX_OUTPUT_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    (text, truncated)
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
    fn a_captured_run_keeps_its_engine_when_the_active_link_changes() {
        static OWNER: External = External {
            name: "fixture-owner",
            env_override: "DS_EXEC_CAPTURE_FIXTURE_UNUSED",
            owner: "owning-regression",
            remedy: "repair fixture",
            missing_code: "fixture_missing",
        };
        let root = std::env::temp_dir().join(format!("ds-exec-capture-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("version-one");
        let second = root.join("version-two");
        for (path, value) in [(&first, "one"), (&second, "two")] {
            fs::write(
                path,
                format!("#!/bin/sh\nprintf x >> \"${{0}}.calls\"\nprintf '%s' '{value}'\n"),
            )
            .unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let active = root.join("fixture-owner");
        std::os::unix::fs::symlink(&first, &active).unwrap();
        let capture = || OWNER.capture_path(OWNER.locate_for_paths(Some(&root.join("ds")), None));
        let held = capture().unwrap();
        assert!(!first.with_extension("calls").exists());
        assert!(!second.with_extension("calls").exists());
        fs::remove_file(&active).unwrap();
        std::os::unix::fs::symlink(&second, &active).unwrap();
        assert_eq!(held.executable(), fs::canonicalize(&first).unwrap());
        assert_eq!(
            held.call("build-info", &[], Duration::from_secs(2))
                .unwrap()
                .stdout,
            "one"
        );
        assert_eq!(
            capture()
                .unwrap()
                .call("build-info", &[], Duration::from_secs(2))
                .unwrap()
                .stdout,
            "two"
        );
        // Removing the active pointer cannot redirect or invalidate an admitted run.
        fs::remove_file(&active).unwrap();
        assert_eq!(
            held.call("build-info", &[], Duration::from_secs(2))
                .unwrap()
                .stdout,
            "one"
        );
        assert!(capture().is_err());
        assert_eq!(fs::read(first.with_extension("calls")).unwrap(), b"xx");
        assert_eq!(fs::read(second.with_extension("calls")).unwrap(), b"x");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_retained_pin_reopens_only_an_absolute_ordinary_executable() {
        static OWNER: External = External {
            name: "fixture-owner",
            env_override: "DS_EXEC_RETAINED_PIN_FIXTURE",
            owner: "owning-regression",
            remedy: "repair fixture",
            missing_code: "fixture_missing",
        };
        let root = std::env::temp_dir().join(format!("ds-exec-pin-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let pinned = root.join("version-one");
        fs::write(&pinned, b"#!/bin/sh\nprintf '%s' 'retained-one'\n").unwrap();
        fs::set_permissions(&pinned, fs::Permissions::from_mode(0o700)).unwrap();
        // An invalid override must neither reject nor redirect the durable pin.
        unsafe { std::env::set_var(OWNER.env_override, root.join("missing-active")) };
        let held = OWNER.capture_pinned(&pinned).unwrap();
        assert_eq!(held.executable(), fs::canonicalize(&pinned).unwrap());
        assert_eq!(
            held.call("build-info", &[], Duration::from_secs(2))
                .unwrap()
                .stdout,
            "retained-one"
        );
        for rejected in [
            Path::new("version-one"),
            root.as_path(),
            root.join("missing").as_path(),
        ] {
            assert_eq!(
                OWNER.capture_pinned(rejected).err().unwrap().code(),
                "fixture_missing"
            );
        }
        let active = root.join("active");
        std::os::unix::fs::symlink(&pinned, &active).unwrap();
        assert_eq!(
            OWNER.capture_pinned(&active).err().unwrap().code(),
            "fixture_missing"
        );
        fs::set_permissions(&pinned, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            OWNER.capture_pinned(&pinned).err().unwrap().code(),
            "fixture_missing"
        );
        unsafe { std::env::remove_var(OWNER.env_override) };
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn canary_cli_uses_only_lane_isolated_packaged_sidecars() {
        let executable = Path::new("/usr/bin/ds-canary");
        assert_eq!(
            packaged_sibling(executable, "ds-solar"),
            Some((PathBuf::from("/usr/bin/ds-solar-canary"), true))
        );
        assert_eq!(
            packaged_sibling(executable, "ds-report"),
            Some((PathBuf::from("/usr/bin/ds-report-canary"), true))
        );
    }

    #[test]
    fn stable_cli_keeps_stable_packaged_sidecars_unsealed() {
        assert_eq!(
            packaged_sibling(Path::new("/usr/bin/ds"), "ds-solar"),
            Some((PathBuf::from("/usr/bin/ds-solar"), false))
        );
    }

    #[test]
    fn missing_canary_sidecar_does_not_fall_back_to_stable_on_path() {
        let root = std::env::temp_dir().join(format!("ds-cli-exec-lane-{}", std::process::id()));
        let stable_bin = root.join("stable-bin");
        fs::create_dir_all(&stable_bin).unwrap();
        let stable_solar = stable_bin.join("ds-solar");
        fs::write(&stable_solar, b"#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = fs::metadata(&stable_solar).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&stable_solar, permissions).unwrap();

        let owner = External {
            name: "ds-solar",
            env_override: "DS_CLI_EXEC_CANARY_MISSING_TEST",
            owner: "test",
            remedy: "repair fixture",
            missing_code: "fixture_missing",
        };
        assert_eq!(
            owner.locate_for_paths(
                Some(&root.join("canary-bin/ds-canary")),
                Some(stable_bin.as_os_str()),
            ),
            None
        );
        fs::remove_dir_all(root).unwrap();
    }

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
        let started = Instant::now();
        assert_eq!(
            owner
                .call_with_stdin("stall", &[], &blocking, Duration::from_millis(80))
                .err()
                .expect("stalled owner must time out")
                .code(),
            "callee_timed_out"
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "inherited stdin or output remained open after the deadline"
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

#[cfg(test)]
mod supervised_tests {
    use super::*;
    static OWNER: External = External {
        name: "native-fixture",
        env_override: "DS_EXEC_NATIVE_FIXTURE_UNUSED",
        owner: "ds-cli-exec-tests",
        remedy: "repair fixture",
        missing_code: "fixture_missing",
    };
    fn captured() -> CapturedExternal {
        OWNER
            .capture_path(Some(std::env::current_exe().unwrap()))
            .unwrap()
    }
    fn fixture(name: &'static str) -> Vec<OsString> {
        vec![
            format!("supervised_tests::{name}").into(),
            "--ignored".into(),
            "--nocapture".into(),
        ]
    }
    fn prompt(started: Instant) {
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "stopped task retained inherited pipes"
        );
    }
    #[test]
    fn supervised_deadline_reaps_native_child() {
        let started = Instant::now();
        let failure = captured()
            .call_cancelled(
                "--exact",
                &fixture("fixture_sleep"),
                Duration::from_millis(80),
                1,
                &|| false,
            )
            .err()
            .expect("stalled fixture must timeout");
        assert_eq!(failure.code(), "callee_timed_out");
        prompt(started);
    }
    #[test]
    fn supervised_cancellation_reaps_native_child() {
        let started = Instant::now();
        let cancel = || started.elapsed() >= Duration::from_millis(80);
        let failure = captured()
            .call_cancelled(
                "--exact",
                &fixture("fixture_sleep"),
                Duration::from_secs(10),
                1,
                &cancel,
            )
            .err()
            .expect("cancelled fixture must stop");
        assert_eq!(failure.code(), "callee_cancelled");
        prompt(started);
    }
    #[test]
    fn completed_leader_reaps_descendant_holding_its_pipes() {
        let started = Instant::now();
        let done = captured()
            .call_cancelled(
                "--exact",
                &fixture("fixture_descendant"),
                Duration::from_secs(2),
                1,
                &|| false,
            )
            .unwrap();
        assert!(done.succeeded());
        assert!(done.stdout.contains("descendant-launched"));
        prompt(started);
    }
    #[test]
    fn output_overflow_stops_tree_and_retains_completed_contract() {
        let started = Instant::now();
        let done = captured()
            .call_cancelled(
                "--exact",
                &fixture("fixture_overflow"),
                Duration::from_secs(10),
                1,
                &|| false,
            )
            .unwrap();
        assert!(done.truncated);
        assert!(done.stdout.len() <= MAX_OUTPUT_BYTES);
        assert!(done.stderr.len() <= MAX_OUTPUT_BYTES);
        prompt(started);
    }
    #[test]
    fn host_worker_budget_caps_thread_pools() {
        let done = captured()
            .call_cancelled(
                "--exact",
                &fixture("fixture_budget"),
                Duration::from_secs(2),
                usize::MAX,
                &|| false,
            )
            .unwrap();
        assert!(done.succeeded());
        let width = std::thread::available_parallelism().map_or(1, usize::from);
        assert!(done.stdout.contains(&format!("worker-width={width}")));
    }
    #[test]
    fn cancellation_before_launch_and_zero_workers_refuse() {
        let started = Instant::now();
        for (workers, cancel, code) in [
            (1, true, "callee_cancelled"),
            (0, false, "callee_worker_budget_invalid"),
        ] {
            let failure = captured()
                .call_cancelled(
                    "--exact",
                    &fixture("fixture_sleep"),
                    Duration::from_secs(30),
                    workers,
                    &|| cancel,
                )
                .err()
                .expect("invalid launch must refuse");
            assert_eq!(failure.code(), code);
        }
        prompt(started);
    }
    #[test]
    fn cancellation_delivers_one_closed_control_and_retains_terminal_output() {
        let started = Instant::now();
        let done = captured()
            .call_cancelled(
                "--exact",
                &fixture("fixture_cancel_ack"),
                Duration::from_secs(2),
                1,
                &|| started.elapsed() >= Duration::from_millis(80),
            )
            .unwrap();
        assert!(done.succeeded());
        assert!(done.stdout.contains("cancel-acknowledged"));
        assert!(!done.truncated);
        prompt(started);
    }
    #[test]
    fn ordinary_call_closes_stdin_and_preserves_nonzero_exit() {
        let done = captured()
            .call(
                "--exact",
                &fixture("fixture_stdin_eof"),
                Duration::from_secs(2),
            )
            .unwrap();
        assert!(done.succeeded());
        assert!(done.stdout.contains("stdin-closed"));
        let refused = captured()
            .call("--exact", &fixture("fixture_crash"), Duration::from_secs(2))
            .unwrap();
        assert_eq!(refused.status, Some(17));
        assert_eq!(
            OWNER.failure_from(&refused, "--exact").code(),
            "engine_refused"
        );
    }
    #[test]
    fn malformed_json_is_a_typed_contract_failure() {
        let failure = captured()
            .call_json(
                "--exact",
                &fixture("fixture_malformed"),
                Duration::from_secs(2),
            )
            .err()
            .expect("protocol prose must not become a result");
        assert_eq!(failure.code(), "callee_contract_mismatch");
    }
    #[test]
    fn stderr_overflow_stops_the_owned_task() {
        let started = Instant::now();
        let done = captured()
            .call_cancelled(
                "--exact",
                &fixture("fixture_stderr_overflow"),
                Duration::from_secs(10),
                1,
                &|| false,
            )
            .unwrap();
        assert!(done.truncated);
        assert!(done.stderr.len() <= MAX_OUTPUT_BYTES);
        prompt(started);
    }
    #[test]
    fn observed_stderr_arrives_before_exit_and_cancellation_reaps_the_task() {
        let observed = std::cell::Cell::new(false);
        let started = Instant::now();
        let failure = captured()
            .call_cancelled_observed(
                "--exact",
                &fixture("fixture_live_progress"),
                Duration::from_secs(10),
                1,
                &|| observed.get(),
                &|chunk| {
                    if chunk
                        .windows(b"owner-progress".len())
                        .any(|bytes| bytes == b"owner-progress")
                    {
                        observed.set(true);
                    }
                },
            )
            .err()
            .expect("observed progress must cancel the running child");
        assert!(
            observed.get(),
            "stderr must be delivered while the child is alive"
        );
        assert_eq!(failure.code(), "callee_cancelled");
        prompt(started);
    }
    #[test]
    fn host_observer_preserves_environment_and_closed_stdin() {
        let done = captured()
            .call_host_observed(
                "--exact",
                &fixture("fixture_host_environment"),
                Duration::from_secs(2),
                &|| false,
                &|_| {},
            )
            .unwrap();
        assert!(done.succeeded(), "{} {}", done.stdout, done.stderr);
        assert!(done.stdout.contains("host-environment-preserved"));
    }
    #[test]
    fn host_progress_cancellation_reaps_tree_and_preserves_bounds() {
        let observed = std::cell::Cell::new(false);
        let started = Instant::now();
        let failure = captured()
            .call_host_observed(
                "--exact",
                &fixture("fixture_live_progress"),
                Duration::from_secs(10),
                &|| observed.get(),
                &|chunk| {
                    if chunk.windows(14).any(|bytes| bytes == b"owner-progress") {
                        observed.set(true);
                    }
                },
            )
            .err()
            .expect("observed cancellation must stop child");
        assert!(observed.get());
        assert_eq!(failure.code(), "callee_cancelled");
        prompt(started);
        let done = captured()
            .call_host_observed(
                "--exact",
                &fixture("fixture_descendant"),
                Duration::from_secs(2),
                &|| false,
                &|_| {},
            )
            .unwrap();
        assert!(done.succeeded());
        prompt(started);
    }
    #[test]
    #[ignore = "invoked by host environment proof"]
    fn fixture_host_environment() {
        let mut input = Vec::new();
        std::io::stdin().read_to_end(&mut input).unwrap();
        assert!(input.is_empty());
        // Cargo's own test environment must survive, unlike an offline engine's
        // deliberately reduced environment.
        assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
        println!("host-environment-preserved");
    }
    #[test]
    fn observed_stderr_keeps_the_output_limit_and_stops_overflow() {
        let count = std::cell::Cell::new(0usize);
        let started = Instant::now();
        let done = captured()
            .call_cancelled_observed(
                "--exact",
                &fixture("fixture_stderr_overflow"),
                Duration::from_secs(10),
                1,
                &|| false,
                &|chunk| count.set(count.get() + chunk.len()),
            )
            .unwrap();
        assert!(done.truncated);
        assert!(count.get() <= MAX_OUTPUT_BYTES);
        assert!(done.stderr.len() <= MAX_OUTPUT_BYTES);
        prompt(started);
    }
    #[test]
    #[ignore = "invoked by the live progress supervision test"]
    fn fixture_live_progress() {
        std::io::stderr().write_all(b"owner-progress\n").unwrap();
        std::io::stderr().flush().unwrap();
        loop {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    #[test]
    fn invalid_utf8_cannot_expand_the_output_bound_or_become_json() {
        let started = Instant::now();
        let done = captured()
            .call(
                "--exact",
                &fixture("fixture_invalid_utf8"),
                Duration::from_secs(2),
            )
            .unwrap();
        assert!(done.truncated);
        assert!(done.stdout.len() <= MAX_OUTPUT_BYTES);
        prompt(started);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn handled_completion_reaps_descendants_and_parent_exit_kills_direct_engine() {
        let done = captured()
            .call(
                "--exact",
                &fixture("fixture_linux_lifecycle_proof"),
                Duration::from_secs(5),
            )
            .unwrap();
        assert!(done.succeeded(), "{} {}", done.stdout, done.stderr);
        assert!(done.stdout.contains("linux-lifecycle-proved"));
    }
    #[test]
    #[ignore]
    fn fixture_cancel_ack() {
        use std::io::BufRead;
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).unwrap();
        assert_eq!(
            line,
            "{\"schema\":\"ds.network.control/v1\",\"operation\":\"cancel\"}\n"
        );
        println!("cancel-acknowledged");
    }
    #[test]
    #[ignore]
    fn fixture_stdin_eof() {
        let mut input = Vec::new();
        std::io::stdin().read_to_end(&mut input).unwrap();
        assert!(input.is_empty());
        println!("stdin-closed");
    }
    #[test]
    #[ignore]
    fn fixture_crash() {
        std::process::exit(17);
    }
    #[test]
    #[ignore]
    fn fixture_malformed() {
        println!("{{\"terminal\":");
    }
    #[test]
    #[ignore]
    fn fixture_stderr_overflow() {
        let _ = std::io::stderr().write_all(&vec![b'y'; MAX_OUTPUT_BYTES + 65536]);
        std::thread::sleep(Duration::from_secs(30));
    }
    #[test]
    #[ignore]
    fn fixture_invalid_utf8() {
        // Leave room for the test harness text: raw bytes fit the bound,
        // but lossy UTF-8 decoding would otherwise expand them threefold.
        let _ = std::io::stdout().write_all(&vec![0xff; MAX_OUTPUT_BYTES - 1024]);
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore]
    fn fixture_linux_lifecycle_proof() {
        // Run the subreaper proof in its own fixture process, keeping the
        // test harness and all unrelated host child ownership unchanged.
        assert_eq!(unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) }, 0);
        let done = captured()
            .call_cancelled(
                "--exact",
                &fixture("fixture_descendant"),
                Duration::from_secs(2),
                1,
                &|| false,
            )
            .unwrap();
        assert!(done.succeeded());
        let pid: libc::pid_t = done
            .stdout
            .lines()
            .find_map(|line| line.strip_prefix("descendant-pid="))
            .expect("descendant receipt")
            .parse()
            .unwrap();
        wait_adopted(pid);

        let root =
            std::env::temp_dir().join(format!("ds-exec-parent-proof-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        // This environment belongs only to the isolated proof process. The
        // bounded Network environment admits its disk-backed scratch path.
        unsafe { std::env::set_var("TMPDIR", &root) };
        for cancel_requested in [false, true] {
            let started = Instant::now();
            let failure = captured()
                .call_cancelled(
                    "--exact",
                    &fixture("fixture_descendant_hang"),
                    if cancel_requested {
                        Duration::from_secs(2)
                    } else {
                        Duration::from_millis(200)
                    },
                    1,
                    &|| cancel_requested && started.elapsed() >= Duration::from_millis(200),
                )
                .err()
                .expect("hung process tree must stop");
            assert_eq!(
                failure.code(),
                if cancel_requested {
                    "callee_cancelled"
                } else {
                    "callee_timed_out"
                }
            );
            let pid = std::fs::read_to_string(root.join("descendant-pid"))
                .unwrap()
                .parse()
                .unwrap();
            wait_adopted(pid);
            std::fs::remove_file(root.join("descendant-pid")).unwrap();
            prompt(started);
        }
        let receipt = root.join("engine-pid");
        struct OwnedParent(std::process::Child);
        impl Drop for OwnedParent {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut parent = OwnedParent(
            Command::new(std::env::current_exe().unwrap())
                .arg("--exact")
                .args(fixture("fixture_parent_supervisor"))
                .env("DS_EXEC_PARENT_PID_RECEIPT", &receipt)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let started = Instant::now();
        let pid: libc::pid_t = loop {
            if let Ok(value) = std::fs::read_to_string(&receipt)
                && let Ok(pid) = value.parse()
            {
                break pid;
            }
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "engine did not start"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        parent.0.kill().unwrap();
        parent.0.wait().unwrap();
        wait_adopted(pid);
        std::fs::remove_dir_all(root).unwrap();
        println!("linux-lifecycle-proved");
    }
    #[cfg(target_os = "linux")]
    fn wait_adopted(pid: libc::pid_t) {
        let started = Instant::now();
        loop {
            let mut status = 0;
            let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if result == pid {
                assert!(libc::WIFSIGNALED(status));
                assert_eq!(libc::WTERMSIG(status), libc::SIGKILL);
                break;
            }
            assert_eq!(result, 0, "owned descendant must be adopted for reaping");
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "owned child remains alive"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore]
    fn fixture_parent_supervisor() {
        captured()
            .call(
                "--exact",
                &fixture("fixture_parent_engine"),
                Duration::from_secs(30),
            )
            .unwrap();
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore]
    fn fixture_descendant_hang() {
        let child = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .args(fixture("fixture_sleep"))
            .spawn()
            .unwrap();
        std::fs::write(
            std::env::temp_dir().join("descendant-pid"),
            child.id().to_string(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_secs(30));
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore]
    fn fixture_parent_engine() {
        std::fs::write(
            std::env::var_os("DS_EXEC_PARENT_PID_RECEIPT").unwrap(),
            std::process::id().to_string(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_secs(30));
    }
    #[test]
    #[ignore]
    fn fixture_sleep() {
        std::thread::sleep(Duration::from_secs(30));
    }
    #[test]
    #[ignore]
    fn fixture_descendant() {
        let child = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .args(fixture("fixture_sleep"))
            .spawn()
            .unwrap();
        println!("descendant-launched");
        println!("descendant-pid={}", child.id());
        std::thread::sleep(Duration::from_millis(80));
    }
    #[test]
    #[ignore]
    fn fixture_overflow() {
        let stdout = std::thread::spawn(|| {
            let _ = std::io::stdout().write_all(&vec![b'x'; MAX_OUTPUT_BYTES + 65536]);
        });
        let _ = std::io::stderr().write_all(&vec![b'y'; MAX_OUTPUT_BYTES + 65536]);
        let _ = stdout.join();
        std::thread::sleep(Duration::from_secs(30));
    }
    #[test]
    #[ignore]
    fn fixture_budget() {
        let width = std::env::var("DS_NETWORK_WORKER_THREADS").unwrap();
        for key in [
            "RAYON_NUM_THREADS",
            "OMP_NUM_THREADS",
            "OPENBLAS_NUM_THREADS",
            "MKL_NUM_THREADS",
            "NUMEXPR_NUM_THREADS",
            "VECLIB_MAXIMUM_THREADS",
        ] {
            assert_eq!(std::env::var(key).unwrap(), width);
        }
        assert!(std::env::var_os("GOOGLE_APPLICATION_CREDENTIALS").is_none());
        assert!(std::env::var_os("LD_PRELOAD").is_none());
        println!("worker-width={width}");
    }
}
