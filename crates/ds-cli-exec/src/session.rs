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
        process_tree::Tree::prepare(&mut command);
        let mut child = command.spawn().map_err(|error| {
            Failure::unavailable(
                self.owner.missing_code,
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
            child,
            tree,
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
