//! `core.store`: one Kernel durable-store call in Core's protected SQLite
//! Store. The Kernel (`ds_command_kernel::durable_store`) owns the call
//! vocabulary, schema, SQL, transaction and refusals; this command binds the
//! protected identity, reads the private request (and payload bytes) and
//! writes the private answer (and answer bytes). No caller SQL, transport,
//! daemon or project selection.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
};
use ds_command_kernel::durable_store::{Call, Identity, MAX_BYTES, Principal};
use ds_sync_store::Store;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const INVALID: Refusal = Refusal {
    code: "core_store_invalid_input",
    when: "the request is not one Kernel durable-store call, or a path is not an absolute private file",
    remedy: "send {request: {store, operation}} from an owner-only request file, with unused absolute output paths",
};
const NOT_SERVED: Refusal = Refusal {
    code: "core_store_not_served",
    when: "the named store has its own Core command",
    remedy: "use the store's own command named in the message (design.status or design.model.catalog)",
};
const STORE: Refusal = Refusal {
    code: "core_store_refused",
    when: "the Kernel store refuses the call, or the protected native Store is unsafe, corrupt or incompatible",
    remedy: "read the Kernel refusal in the message; nothing was written when the store refused",
};
const OUTPUT: Refusal = Refusal {
    code: "core_store_output_invalid",
    when: "an absolute new output cannot be created privately or the answer exceeds 64 MiB",
    remedy: "use unused absolute output files in an existing protected directory; if detail says mutation_completed, read the store before retrying",
};
const CHANGED: Refusal = Refusal {
    code: "core_store_identity_changed",
    when: "the protected account or lane changes before the call or its acknowledgment",
    remedy: "restore the intended protected identity and read the store before retrying a completed write",
};

pub static COMMAND: Command = Command {
    id: "core.store",
    path: &["core", "store"],
    contract: 1,
    summary: "Run one Kernel durable-store call in Core's protected Store.",
    purpose: "Run one Kernel durable-store call ({request: {store, operation}, expect_owner?}) in one transaction of the lane's protected Store under the protected account, bound here and never read from the request. The Kernel owns stores, schema, SQL and refusals; a store with its own command is refused by name. --bytes and --bytes-out carry payload bytes. The answer goes to a new private file (at most 64 MiB) after an identity recheck.",
    chapter: Chapter::Workstation,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        super::LANE.required(),
        Arg::value(
            "request",
            "<absolute-existing-json-file>",
            "Owner-only ordinary file, at most 64 MiB: the Kernel Call JSON {\"request\": {\"store\", \"operation\"}, \"expect_owner\"?}.",
        )
        .required(),
        Arg::value(
            "out",
            "<absolute-new-json-file>",
            "New owner-only answer file in an existing protected directory; never replaced, at most 64 MiB.",
        )
        .required(),
        Arg::value(
            "bytes",
            "<absolute-existing-file>",
            "Owner-only payload bytes for a byte-carrying store operation, at most 64 MiB.",
        ),
        Arg::value(
            "bytes-out",
            "<absolute-new-file>",
            "New owner-only file for the answer's bytes; written only when the store answers bytes.",
        ),
    ],
    output: "Receipt ds.core-store-file/v1: path, sha256, size_bytes, store, lane, owner (account) and bytes ({path, sha256, size_bytes} or null). The file is ds.core-store/v1 with the same store, lane and owner and the Kernel answer as result (its generation and owner fence included).",
    examples: &[],
    refusals: &[super::PLATFORM, INVALID, NOT_SERVED, STORE, OUTPUT, CHANGED],
    reference: Some("docs/reference/core.md"),
    search: &["sqlite", "preferences"],
    requires: Requires::Server,
    availability: super::native_host_availability,
};

fn failure(refusal: Refusal, detail: impl ToString) -> Failure {
    Failure::failed(refusal.code, detail.to_string()).remedy(refusal.remedy)
}
fn absolute(path: &Path) -> Result<(), Failure> {
    if !path.is_absolute() || path.to_str().is_none() {
        return Err(failure(INVALID, "every path must be an absolute Unicode path"));
    }
    Ok(())
}
/// Read one owner-only ordinary file of at most 64 MiB.
fn private_input(path: &Path) -> Result<Vec<u8>, Failure> {
    absolute(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|e| failure(INVALID, e))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(failure(INVALID, "an input must be an ordinary file, never a symlink"));
    }
    let mut file = super::cache_recovery::private_file(path).map_err(|e| failure(INVALID, e))?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| failure(INVALID, e))?;
    if bytes.len() > MAX_BYTES {
        return Err(failure(INVALID, "an input exceeds 64 MiB"));
    }
    Ok(bytes)
}
fn call(path: &Path) -> Result<Call, Failure> {
    let call: Call = serde_json::from_slice(&private_input(path)?).map_err(|e| failure(INVALID, e))?;
    call.request.core_homed().map_err(|e| failure(NOT_SERVED, e))?;
    Ok(call)
}
fn create_output(path: &Path) -> Result<fs::File, Failure> {
    absolute(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| failure(OUTPUT, "output has no parent"))?;
    super::host::validate_directory(parent).map_err(|e| failure(OUTPUT, e))?;
    #[cfg(windows)]
    {
        ds_cli_auth::windows_host::create_private_output_file(path).map_err(|e| failure(OUTPUT, e))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|e| failure(OUTPUT, e))
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err(failure(OUTPUT, "private native files unavailable"))
    }
}
/// The identity the Kernel binds into every Core-homed row: the protected
/// account, the lane, and the canonical (account, lane, audience) owner fence.
fn identity(lane: &str) -> Result<Identity, Failure> {
    let principal = ds_cli_auth::headless_principal_read_only(lane)?;
    Ok(Identity {
        principal: Principal::Account(principal.account_uid().into()),
        lane: lane.into(),
        audience: principal.job_owner()?,
    })
}
fn perform(database: &Path, identity: &Identity, call: Call, bytes: Option<Vec<u8>>) -> Result<ds_command_kernel::durable_store::Answer, Failure> {
    let parent = database
        .parent()
        .ok_or_else(|| failure(STORE, "store has no parent"))?;
    super::host::prepare_directory(parent).map_err(|e| failure(STORE, e))?;
    let mut store = Store::open(database).map_err(|e| failure(STORE, e))?;
    store
        .durable_store(identity, call, bytes)
        .map_err(|e| failure(STORE, e))
}
fn write_private(file: &mut fs::File, bytes: &[u8]) -> Result<(), Failure> {
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| failure(OUTPUT, e))
}
fn capture(
    database: &Path,
    lane: &str,
    call: Call,
    bytes: Option<Vec<u8>>,
    out: &Path,
    bytes_out: Option<&Path>,
    observe: impl Fn() -> Result<Identity, Failure>,
) -> Result<Value, Failure> {
    let identity = observe()?;
    identity.validate().map_err(|e| failure(INVALID, e))?;
    let store = call.request.store();
    let account = match &identity.principal {
        Principal::Account(uid) => uid.clone(),
        Principal::Anonymous => return Err(failure(CHANGED, "Core binds a signed-in account")),
    };
    let owner = json!({"account": account});
    // Reserve the actual owner-only files before any store effect.
    let mut file = create_output(out)?;
    let mut bytes_file = bytes_out.map(create_output).transpose()?;
    let mut completed = false;
    let result = (|| {
        if observe()? != identity {
            return Err(failure(CHANGED, "protected identity changed before the store call"));
        }
        let answer = perform(database, &identity, call, bytes)?;
        completed = true;
        let answer_bytes = answer.bytes.clone();
        let payload = json!({"schema":"ds.core-store/v1","store":store,"lane":lane,"owner":owner,"result":answer});
        let text = serde_json::to_vec(&payload).map_err(|e| failure(OUTPUT, e))?;
        if text.len() > MAX_BYTES {
            return Err(failure(OUTPUT, "the answer exceeds 64 MiB"));
        }
        write_private(&mut file, &text)?;
        let bytes_receipt = match (answer_bytes, bytes_file.as_mut(), bytes_out) {
            (Some(payload), Some(target), Some(path)) => {
                write_private(target, &payload)?;
                json!({"path":path,"sha256":format!("{:x}", Sha256::digest(&payload)),"size_bytes":payload.len()})
            }
            (Some(_), _, _) => return Err(failure(OUTPUT, "the store answered bytes; pass --bytes-out")),
            (None, _, _) => Value::Null,
        };
        if observe()? != identity {
            return Err(failure(CHANGED, "protected identity changed before the store acknowledgment"));
        }
        Ok(json!({"schema":"ds.core-store-file/v1","path":out,"sha256":format!("{:x}", Sha256::digest(&text)),"size_bytes":text.len(),"store":store,"lane":lane,"owner":owner,"bytes":bytes_receipt}))
    })();
    drop(file);
    drop(bytes_file);
    match result {
        Ok(receipt) => Ok(receipt),
        Err(error) => {
            let mut cleanup = vec![fs::remove_file(out).err().map(|e| e.to_string())];
            if let Some(path) = bytes_out {
                cleanup.push(fs::remove_file(path).err().map(|e| e.to_string()));
            }
            let cleanup_error = cleanup.into_iter().flatten().next();
            if completed || cleanup_error.is_some() {
                return Err(failure(OUTPUT, if completed { "store call committed; output acknowledgment failed" } else { "store call failed and unacknowledged output cleanup failed" })
                    .detail(json!({"mutation_completed":completed,"store":store,"lane":lane,"cause":error.code(),"cause_message":error.message(),"cleanup_error":cleanup_error})));
            }
            Err(error)
        }
    }
}
pub fn command(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    super::require_native_host()?;
    let lane = inputs.require("lane")?;
    let call = call(&PathBuf::from(inputs.require("request")?))?;
    let bytes = inputs
        .value("bytes")
        .map(|path| private_input(&PathBuf::from(path)))
        .transpose()?;
    let bytes_out = inputs.value("bytes-out").map(PathBuf::from);
    let database = ds_compute_runtime::server_state_directory(lane, None)
        .map_err(|e| failure(STORE, e))?
        .join("store.sqlite");
    capture(
        &database,
        lane,
        call,
        bytes,
        &PathBuf::from(inputs.require("out")?),
        bytes_out.as_deref(),
        || identity(lane),
    )
}

#[cfg(test)]
#[path = "durable_store_tests.rs"]
mod tests;
