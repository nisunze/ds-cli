//! Fixed, retained Network Profile transport using the existing process owner.
use super::*;
use std::{
    io::{BufRead, BufReader},
    process::Child,
    sync::mpsc,
    thread::JoinHandle,
};

const FRAME_BYTES: usize = 64 * 1024;
const CANCEL_GRACE: Duration = Duration::from_millis(300);
enum Input {
    Frame(Vec<u8>, mpsc::SyncSender<bool>),
    Eof,
}
enum Output {
    Frame(Vec<u8>),
    Eof,
    Invalid,
}

// Linux parent-death signals follow the creating OS thread, not merely its
// process. A retained worker therefore cannot be launched by an ephemeral
// caller/Tokio blocking thread. Keep its private creator alive through reap.
#[cfg(target_os = "linux")]
struct LauncherThread {
    release: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}
#[cfg(target_os = "linux")]
impl Drop for LauncherThread {
    fn drop(&mut self) {
        self.release.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
#[cfg(target_os = "linux")]
struct RetainedLaunch {
    child: Option<Child>,
    tree: Option<process_tree::Tree>,
    launcher: Option<LauncherThread>,
}
#[cfg(target_os = "linux")]
impl Drop for RetainedLaunch {
    fn drop(&mut self) {
        // Also fences constructor unwind before pipes/collectors are complete.
        if let (Some(child), Some(tree)) = (&mut self.child, &mut self.tree) {
            tree.terminate();
            let _ = child.kill();
            let _ = child.wait();
        }
        self.launcher.take();
    }
}
fn spawn_profile(
    mut command: Command,
    missing_code: &'static str,
) -> Result<(Child, process_tree::Tree), Failure> {
    process_tree::Tree::prepare(&mut command);
    let mut child = command.spawn().map_err(|error| {
        Failure::unavailable(
            missing_code,
            "could not start the captured Network Profile engine",
        )
        .detail(json!({"detail":error.kind().to_string()}))
    })?;
    let tree = match process_tree::Tree::attach(&mut child) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Failure::internal(
                "callee_supervision_failed",
                "could not own the Profile process tree",
            )
            .detail(json!({"detail":error.kind().to_string()})));
        }
    };
    Ok((child, tree))
}
#[cfg(target_os = "linux")]
fn hold_launched_profile(
    command: Command,
    missing_code: &'static str,
    ready: mpsc::SyncSender<Result<(Child, process_tree::Tree), Failure>>,
    held: mpsc::Receiver<()>,
) {
    match ready.send(spawn_profile(command, missing_code)) {
        Ok(()) => {
            let _ = held.recv();
        }
        Err(mpsc::SendError(Ok((mut child, mut tree)))) => {
            // Receiver abandonment must reap before this creator thread exits.
            tree.terminate();
            let _ = child.kill();
            let _ = child.wait();
        }
        Err(mpsc::SendError(Err(_))) => {}
    }
}
#[cfg(target_os = "linux")]
fn launch_retained(
    command: Command,
    missing_code: &'static str,
) -> Result<RetainedLaunch, Failure> {
    let (ready, received) = mpsc::sync_channel(1);
    let (release, held) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("ds-profile-launch-owner".into())
        .spawn(move || hold_launched_profile(command, missing_code, ready, held))
        .map_err(|error| {
            Failure::internal(
                "callee_supervision_failed",
                "could not retain the Profile launch thread",
            )
            .detail(json!({"detail":error.kind().to_string()}))
        })?;
    let launcher = LauncherThread {
        release: Some(release),
        thread: Some(thread),
    };
    let (child, tree) = received.recv().map_err(|_| {
        Failure::internal(
            "callee_supervision_failed",
            "Profile launch owner ended before handoff",
        )
    })??;
    Ok(RetainedLaunch {
        child: Some(child),
        tree: Some(tree),
        launcher: Some(launcher),
    })
}

/// One worker-affine Network Profile process. Frames are bounded, queues retain
/// at most one frame, and every termination reaps the owned process tree before
/// joining pipe collectors. No active executable lookup occurs after launch.
pub struct NetworkProfileProcess {
    child: Child,
    tree: process_tree::Tree,
    input: Option<mpsc::SyncSender<Input>>,
    output: Option<mpsc::Receiver<Output>>,
    writer: Option<JoinHandle<()>>,
    reader: Option<JoinHandle<()>>,
    stderr: Option<JoinHandle<(String, bool)>>,
    overflow: Arc<AtomicBool>,
    stopped: bool,
    #[cfg(target_os = "linux")]
    launcher: Option<LauncherThread>,
}
impl CapturedExternal {
    /// Open only the declared offline Profile owner; arguments are constructed
    /// here from typed paths and a worker budget, never caller-provided argv.
    pub fn network_profile_session(
        &self,
        request: &Path,
        out: &Path,
        workers: usize,
    ) -> Result<NetworkProfileProcess, Failure> {
        if workers == 0 || workers > std::thread::available_parallelism().map_or(1, usize::from) {
            return Err(Failure::invalid(
                "callee_worker_budget_invalid",
                "invalid Profile worker budget",
            ));
        }
        self.verify_managed()?;
        let mut command = Command::new(&self.executable);
        command
            .arg("profile-session")
            .arg("--request")
            .arg(request)
            .arg("--out")
            .arg(out)
            .arg("--workers")
            .arg(workers.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
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
        for key in [
            "DS_NETWORK_WORKER_THREADS",
            "RAYON_NUM_THREADS",
            "OMP_NUM_THREADS",
            "OPENBLAS_NUM_THREADS",
            "MKL_NUM_THREADS",
            "NUMEXPR_NUM_THREADS",
            "VECLIB_MAXIMUM_THREADS",
        ] {
            command.env(key, workers.to_string());
        }
        #[cfg(target_os = "linux")]
        let mut launch = launch_retained(command, self.owner.missing_code)?;
        #[cfg(target_os = "linux")]
        let child = launch.child.as_mut().expect("owned child");
        #[cfg(not(target_os = "linux"))]
        let (mut child, tree) = spawn_profile(command, self.owner.missing_code)?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take();
        let overflow = Arc::new(AtomicBool::new(false));
        let (input, inputs) = mpsc::sync_channel(1);
        let (outputs, output) = mpsc::sync_channel(1);
        let writer = std::thread::spawn(move || {
            let mut stdin = stdin;
            while let Ok(input) = inputs.recv() {
                match input {
                    Input::Frame(bytes, done) => {
                        let ok = stdin
                            .write_all(&bytes)
                            .and_then(|_| stdin.write_all(b"\n"))
                            .and_then(|_| stdin.flush())
                            .is_ok();
                        let _ = done.send(ok);
                        if !ok {
                            break;
                        }
                    }
                    Input::Eof => break,
                }
            }
        });
        let stdout_overflow = Arc::clone(&overflow);
        let reader = std::thread::spawn(move || {
            let mut stream = BufReader::new(stdout);
            loop {
                let mut bytes = Vec::new();
                let result = stream
                    .by_ref()
                    .take(FRAME_BYTES as u64 + 1)
                    .read_until(b'\n', &mut bytes);
                let frame = match result {
                    Ok(0) => Output::Eof,
                    Ok(_) if bytes.len() <= FRAME_BYTES && bytes.last() == Some(&b'\n') => {
                        bytes.pop();
                        Output::Frame(bytes)
                    }
                    _ => {
                        stdout_overflow.store(true, Ordering::Release);
                        Output::Invalid
                    }
                };
                let terminal = !matches!(frame, Output::Frame(_));
                // A bounded channel prevents unsolicited stdout from growing
                // host memory; dropping the receiver releases this sender.
                if outputs.send(frame).is_err() || terminal {
                    break;
                }
            }
        });
        let stderr_overflow = Arc::clone(&overflow);
        let stderr = std::thread::spawn(move || read_bounded_signal(stderr, &stderr_overflow));
        Ok(NetworkProfileProcess {
            #[cfg(target_os = "linux")]
            child: launch.child.take().expect("owned child"),
            #[cfg(not(target_os = "linux"))]
            child,
            #[cfg(target_os = "linux")]
            tree: launch.tree.take().expect("owned tree"),
            #[cfg(not(target_os = "linux"))]
            tree,
            #[cfg(target_os = "linux")]
            launcher: launch.launcher.take(),
            input: Some(input),
            output: Some(output),
            writer: Some(writer),
            reader: Some(reader),
            stderr: Some(stderr),
            overflow,
            stopped: false,
        })
    }
}
impl NetworkProfileProcess {
    pub fn process_id(&self) -> u32 {
        self.child.id()
    }
    pub fn receive(
        &mut self,
        timeout: Duration,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Vec<u8>, Failure> {
        self.receive_until(Instant::now() + timeout, cancel)
    }
    pub fn request(
        &mut self,
        bytes: &[u8],
        timeout: Duration,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Vec<u8>, Failure> {
        if bytes.is_empty()
            || bytes.len() + 1 > FRAME_BYTES
            || bytes.contains(&b'\n')
            || serde_json::from_slice::<serde_json::Value>(bytes).is_err()
        {
            return Err(Failure::invalid(
                "callee_input_bounded",
                "Profile requires one bounded typed JSON frame",
            ));
        }
        self.ensure_running()?;
        let deadline = Instant::now() + timeout;
        self.check_stop(deadline, cancel)?;
        let (done, written) = mpsc::sync_channel(1);
        if self
            .input
            .as_ref()
            .expect("running input")
            .try_send(Input::Frame(bytes.to_vec(), done))
            .is_err()
        {
            self.shutdown();
            return Err(Failure::failed(
                "callee_stdin_failed",
                "Profile input transport is unavailable",
            ));
        }
        loop {
            self.check_stop(deadline, cancel)?;
            match written.recv_timeout(POLL) {
                Ok(true) => break,
                Ok(false) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.shutdown();
                    return Err(Failure::failed(
                        "callee_stdin_failed",
                        "Profile did not consume its complete frame",
                    ));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        self.receive_until(deadline, cancel)
    }
    fn receive_until(
        &mut self,
        deadline: Instant,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Vec<u8>, Failure> {
        self.ensure_running()?;
        loop {
            self.check_stop(deadline, cancel)?;
            match self
                .output
                .as_ref()
                .expect("running output")
                .recv_timeout(POLL)
            {
                Ok(Output::Frame(bytes)) => return Ok(bytes),
                Ok(Output::Invalid) => {
                    self.shutdown();
                    return Err(Failure::failed(
                        "callee_output_bounded",
                        "Profile emitted an invalid or oversized frame",
                    ));
                }
                Ok(Output::Eof) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.shutdown();
                    return Err(Failure::failed(
                        "callee_session_closed",
                        "Profile closed without the expected reply",
                    ));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
    fn ensure_running(&self) -> Result<(), Failure> {
        if self.stopped {
            Err(Failure::failed(
                "callee_session_closed",
                "Profile process has been reaped",
            ))
        } else {
            Ok(())
        }
    }
    fn check_stop(&mut self, deadline: Instant, cancel: &dyn Fn() -> bool) -> Result<(), Failure> {
        if cancel() {
            self.cancel();
            return Err(Failure::failed(
                "callee_cancelled",
                "Profile session was cancelled",
            ));
        }
        if self.overflow.load(Ordering::Acquire) {
            self.shutdown();
            return Err(Failure::failed(
                "callee_output_bounded",
                "Profile exceeded its bounded output contract",
            ));
        }
        if Instant::now() >= deadline {
            self.shutdown();
            return Err(Failure::failed(
                "callee_timed_out",
                "Profile request exceeded its deadline",
            ));
        }
        Ok(())
    }
    /// EOF after an acknowledged Close, then bounded wait and descendant cleanup.
    pub fn finish(&mut self, timeout: Duration, cancel: &dyn Fn() -> bool) -> Result<(), Failure> {
        self.ensure_running()?;
        if let Some(input) = &self.input {
            let _ = input.try_send(Input::Eof);
        }
        let deadline = Instant::now() + timeout;
        let status = loop {
            self.check_stop(deadline, cancel)?;
            match self.tree.try_wait(&mut self.child) {
                Ok(Some(status)) => break status,
                Ok(None) => std::thread::sleep(POLL),
                Err(_) => {
                    self.shutdown();
                    return Err(Failure::failed(
                        "callee_wait_failed",
                        "could not wait for Profile completion",
                    ));
                }
            }
        };
        self.shutdown();
        if status.success() {
            Ok(())
        } else {
            Err(Failure::failed(
                "callee_session_failed",
                "Profile exited unsuccessfully",
            ))
        }
    }
    pub fn cancel(&mut self) {
        if self.stopped {
            return;
        }
        if let Some(input) = &self.input {
            let (done, _written) = mpsc::sync_channel(1);
            let _ = input.try_send(Input::Frame(
                b"{\"schema\":\"ds.network.control/v1\",\"operation\":\"cancel\"}".to_vec(),
                done,
            ));
        }
        let deadline = Instant::now() + CANCEL_GRACE;
        while Instant::now() < deadline {
            if !matches!(self.tree.try_wait(&mut self.child), Ok(None)) {
                break;
            }
            std::thread::sleep(POLL);
        }
        self.shutdown();
    }
    pub fn abort(&mut self) {
        self.shutdown();
    }
    fn shutdown(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        self.tree.terminate();
        let _ = self.child.kill();
        let _ = self.child.wait();
        #[cfg(target_os = "linux")]
        self.launcher.take();
        self.input.take();
        self.output.take();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        if let Some(stderr) = self.stderr.take() {
            let _ = stderr.join();
        }
    }
}
impl Drop for NetworkProfileProcess {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn retained_transport_can_move_between_host_workers() {
        fn assert_send<T: Send>() {}
        assert_send::<super::NetworkProfileProcess>();
    }
}

#[cfg(all(test, target_os = "linux"))]
mod launcher_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    static OWNER: External = External {
        name: "profile-owner-fixture",
        env_override: "DS_PROFILE_FIXTURE_UNUSED",
        owner: "ds-cli-exec-tests",
        remedy: "repair fixture",
        missing_code: "fixture_missing",
    };
    struct Fixture {
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "ds-retained-owner-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::fs::write(
                root.join("engine"),
                r#"#!/usr/bin/python3
import sys,os,pathlib,json
request=pathlib.Path(sys.argv[sys.argv.index('--request')+1])
request.with_suffix('.pid').write_text(str(os.getpid()))
print('ready',flush=True)
for line in sys.stdin:
 if json.loads(line).get('operation')=='close':
  print('closed',flush=True);break
 print(line.strip(),flush=True)
"#,
            )
            .unwrap();
            std::fs::set_permissions(root.join("engine"), std::fs::Permissions::from_mode(0o700))
                .unwrap();
            Self { root }
        }
        fn command(&self, name: &str) -> Command {
            let mut command = Command::new(self.root.join("engine"));
            command
                .args(["profile-session", "--request"])
                .arg(self.root.join(name))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            command
        }
        fn process(&self, name: &str) -> NetworkProfileProcess {
            OWNER
                .capture_pinned(&self.root.join("engine"))
                .unwrap()
                .network_profile_session(&self.root.join(name), &self.root, 1)
                .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn ready(process: &mut NetworkProfileProcess) {
        assert_eq!(
            process.receive(Duration::from_secs(2), &|| false).unwrap(),
            b"ready"
        );
    }
    fn gone(pid: u32) {
        let mut status = 0;
        assert_eq!(
            unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
            -1,
            "owner did not reap its child"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
        assert_eq!(
            unsafe { libc::kill(pid as i32, 0) },
            -1,
            "owned child remains alive or unreaped"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
    #[test]
    fn temporary_caller_exit_keeps_workers_independent_until_close_or_abort() {
        let fixture = Fixture::new();
        let launch = |name: &str| {
            let captured = OWNER.capture_pinned(&fixture.root.join("engine")).unwrap();
            let request = fixture.root.join(name);
            let output = fixture.root.clone();
            std::thread::spawn(move || {
                captured
                    .network_profile_session(&request, &output, 1)
                    .unwrap()
            })
            .join()
            .unwrap()
        };
        let mut first = launch("first");
        let mut second = launch("second");
        ready(&mut first);
        ready(&mut second);
        assert_eq!(
            first
                .request(br#"{}"#, Duration::from_secs(2), &|| false)
                .unwrap(),
            br#"{}"#
        );
        let first_pid = first.process_id();
        let second_pid = second.process_id();
        assert_eq!(
            first
                .request(br#"{"operation":"close"}"#, Duration::from_secs(2), &|| {
                    false
                })
                .unwrap(),
            b"closed"
        );
        first.finish(Duration::from_secs(2), &|| false).unwrap();
        gone(first_pid);
        assert_eq!(
            second
                .request(br#"{"still_owned":true}"#, Duration::from_secs(2), &|| {
                    false
                })
                .unwrap(),
            br#"{"still_owned":true}"#
        );
        second.abort();
        gone(second_pid);
    }
    #[test]
    fn drop_cancel_and_constructor_unwind_reap_before_releasing_creator() {
        let fixture = Fixture::new();
        let mut cancelled = fixture.process("cancel");
        ready(&mut cancelled);
        let pid = cancelled.process_id();
        cancelled.cancel();
        gone(pid);
        let mut dropped = fixture.process("drop");
        ready(&mut dropped);
        let pid = dropped.process_id();
        drop(dropped);
        gone(pid);
        let mut pid = 0;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let launch = launch_retained(fixture.command("unwind"), "fixture_missing").unwrap();
            pid = launch.child.as_ref().unwrap().id();
            panic!("synthetic constructor unwind");
        }));
        assert!(result.is_err());
        gone(pid);
    }
    #[test]
    fn disconnected_handoff_reaps_before_launcher_thread_returns() {
        let fixture = Fixture::new();
        let command = fixture.command("abandoned");
        let (send, receive) = mpsc::sync_channel(0);
        let (_release, held) = mpsc::channel();
        let owner = std::thread::spawn(move || {
            hold_launched_profile(command, "fixture_missing", send, held)
        });
        let path = fixture.root.join("abandoned.pid");
        let deadline = Instant::now() + Duration::from_secs(2);
        while !path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let pid = std::fs::read_to_string(path).unwrap().parse().unwrap();
        drop(receive);
        owner.join().unwrap();
        gone(pid);
    }
    #[test]
    fn failed_spawn_releases_creator_without_child_or_hang() {
        let fixture = Fixture::new();
        let started = Instant::now();
        let result = launch_retained(Command::new(fixture.root.join("absent")), "fixture_missing");
        assert_eq!(result.err().unwrap().code(), "fixture_missing");
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    #[test]
    #[ignore = "private subprocess control invoked by retained_parent_death_still_kills_worker"]
    fn fixture_parent_process() {
        let fixture = Fixture {
            root: std::env::var_os("DS_PROFILE_OWNER_PARENT_ROOT")
                .unwrap()
                .into(),
        };
        let mut process = fixture.process("parent");
        ready(&mut process);
        std::fs::write(
            fixture.root.join("owned-child.pid"),
            process.process_id().to_string(),
        )
        .unwrap();
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    #[test]
    fn retained_parent_death_still_kills_worker() {
        let fixture = Fixture::new();
        let mut parent = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "session::launcher_tests::fixture_parent_process",
                "--ignored",
                "--nocapture",
            ])
            .env("DS_PROFILE_OWNER_PARENT_ROOT", &fixture.root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let path = fixture.root.join("owned-child.pid");
        let deadline = Instant::now() + Duration::from_secs(3);
        while !path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let pid_result = std::fs::read_to_string(path).map(|s| s.parse::<u32>().unwrap());
        parent.kill().unwrap();
        parent.wait().unwrap();
        let pid = pid_result.unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                Ok(stat) if stat.split(") ").nth(1).unwrap().starts_with("Z ") => {
                    assert_eq!(
                        stat.split(") ").nth(1).unwrap().split_whitespace().nth(49),
                        Some("9")
                    );
                    break;
                }
                _ if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
                _ => {
                    unsafe {
                        libc::kill(pid as i32, libc::SIGKILL);
                    }
                    panic!("worker survived its parent process");
                }
            }
        }
    }
}
