//! One immutable executable selection shared by an accepted run or session.
use super::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkEnginePin {
    pub schema: String,
    pub executable: PathBuf,
    pub executable_sha256: String,
    pub identity: EngineIdentity,
    pub development_override: bool,
}

pub struct CapturedNetworkClient {
    pub(crate) engine: CapturedExternal,
    pub(crate) identity: EngineIdentity,
    pub(crate) executable_sha256: String,
    pub(crate) development_override: bool,
}

impl CapturedNetworkClient {
    pub fn capture(workers: usize, cancel: &dyn Fn() -> bool) -> Result<Self, NetworkFailure> {
        let (engine, identity, executable_sha256, development_override) =
            capture_identity(workers, cancel)?;
        Ok(Self {
            engine,
            identity,
            executable_sha256,
            development_override,
        })
    }

    /// Capture only the explicit installed release lane. Development selection
    /// remains explicit; managed receipt errors never select another lane.
    pub fn capture_for_lane(
        lane: &str,
        workers: usize,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self, NetworkFailure> {
        Self::capture_for_lane_in(lane, workers, cancel, None)
    }

    /// Native-owned relocated addon store, never an authored IPC path.
    /// None uses the machine's normal protected addon base.
    pub fn capture_for_lane_in(
        lane: &str,
        workers: usize,
        cancel: &dyn Fn() -> bool,
        module_base: Option<&Path>,
    ) -> Result<Self, NetworkFailure> {
        if workers == 0 {
            return Err(NetworkFailure::new(
                "network_workers_invalid",
                "worker budget must be positive",
            ));
        }
        let width = workers.min(std::thread::available_parallelism().map_or(1, usize::from));
        let selected = match module_base {
            Some(base) => NETWORK.capture_for_lane_in(lane, base),
            None => NETWORK.capture_for_lane(lane),
        };
        let engine = selected.map_err(|error| {
            let code = if error.code() == "module_receipt_invalid" {
                "network_engine_incompatible"
            } else {
                static_code(error.code())
            };
            NetworkFailure::new(code, error.to_string())
        })?;
        let development_override = admit_candidate(&engine)?;
        let (engine, identity, executable_sha256, development_override) =
            capture_resolved_identity(engine, width, development_override, cancel)?;
        Ok(Self {
            engine,
            identity,
            executable_sha256,
            development_override,
        })
    }

    pub fn identity(&self) -> &EngineIdentity {
        &self.identity
    }
    pub fn executable(&self) -> &Path {
        self.engine.executable()
    }
    pub fn executable_sha256(&self) -> &str {
        &self.executable_sha256
    }
    pub fn development_override(&self) -> bool {
        self.development_override
    }

    pub fn pin(&self) -> NetworkEnginePin {
        NetworkEnginePin {
            schema: "ds.network.captured-engine/v1".into(),
            executable: self.engine.executable().to_path_buf(),
            executable_sha256: self.executable_sha256.clone(),
            identity: self.identity.clone(),
            development_override: self.development_override,
        }
    }

    /// A retained run never resolves the current module pointer again.
    pub fn from_pin(
        pin: &NetworkEnginePin,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self, NetworkFailure> {
        if pin.schema != "ds.network.captured-engine/v1"
            || !pin.executable.is_absolute()
            || pin.executable.as_os_str().as_encoded_bytes().len() > 4096
            || !valid_hex(&pin.executable_sha256, 64)
        {
            return Err(NetworkFailure::new(
                "network_engine_pin_invalid",
                "invalid captured engine pin",
            ));
        }
        let width = pin.identity.runtime.internal_worker_threads;
        validate_identity(&pin.identity, width)?;
        if width == 0 || width > std::thread::available_parallelism().map_or(1, usize::from) {
            return Err(NetworkFailure::new(
                "network_workers_invalid",
                "captured worker budget is unavailable",
            ));
        }
        let engine = NETWORK
            .capture_pinned(&pin.executable)
            .map_err(|e| NetworkFailure::new(static_code(e.code()), e.to_string()))?;
        if engine.executable() != pin.executable
            || digest_file(
                engine.executable(),
                MAX_EXECUTABLE_BYTES,
                "network_engine_changed",
            )? != pin.executable_sha256
        {
            return Err(NetworkFailure::new(
                "network_engine_changed",
                "retained executable differs from its captured path or digest",
            ));
        }
        let (engine, identity, executable_sha256, development_override) =
            capture_resolved_identity(engine, width, pin.development_override, cancel)?;
        let mut expected = pin.identity.clone();
        // CPU availability is a host capacity fact; immutable engineering pins
        // and the captured worker width remain exact across a host restart.
        expected.runtime.logical_cpus = identity.runtime.logical_cpus;
        if !same_identity(&expected, &identity)? || executable_sha256 != pin.executable_sha256 {
            return Err(NetworkFailure::new(
                "network_engine_changed",
                "retained engine identity changed",
            ));
        }
        Ok(Self {
            engine,
            identity,
            executable_sha256,
            development_override,
        })
    }

    pub fn process_lv_with_order(
        &self,
        input: &[u8],
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
        object_order: ds_network_contracts::engine::JsonObjectOrder,
    ) -> Result<LvOutcome, NetworkFailure> {
        let request = decode_native_lv_request(input).map_err(NetworkFailure::native)?;
        process_lv_captured(self, input, request, scratch, cancel, object_order)
    }
}
