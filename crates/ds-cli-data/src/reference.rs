//! Closed, offline read adapter for the existing global reference cache owner.
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_command_kernel::reference_cache::{self as policy, Identity};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_REQUEST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RESULT_BYTES: usize = 64 * 1024 * 1024;
const INVALID: Refusal = Refusal {
    code: "reference_read_invalid",
    when: "the request file, action, closed fields, identity, area, limit or new output path is invalid",
    remedy: "use one declared read request in an ordinary JSON file and a fresh absolute output path",
};
const UNAVAILABLE: Refusal = Refusal {
    code: "reference_read_unavailable",
    when: "the configured reference cache cannot answer the read or its result cannot be written",
    remedy: "inspect the local cache installation and file permissions, then retry the same read",
};

pub static COMMAND: Command = Command {
    id: "data.reference.read",
    path: &["data", "reference", "read"],
    contract: 1,
    summary: "Read installed reference cache metadata and bounded features offline.",
    purpose: "Reads the configured shared reference cache with no account, project, network or paired Desktop. --request is an ordinary JSON file up to 16 MiB: action storage_status has no other fields; status takes identity {resource_id,version}; versions takes identity {resource_id,version?} (any supplied version is validated and retained); query takes identity, area (GeoJSON Polygon/MultiPolygon), optional limit (1-10000; omitted 5000). Resource/version identities are exact lowercase SHA-256 hex. Unknown fields and every write action refuse before calling storage. --out writes the exact serialized owner result to a new absolute JSON file, bounded to 64 MiB, for results too large for process stdout.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "request",
            "<request.json>",
            "Closed read request JSON in an ordinary local file, at most 16 MiB.",
        )
        .required(),
        Arg::value(
            "out",
            "<absolute-new.json>",
            "Optional new ordinary JSON result file, at most 64 MiB; never clobbers or follows linked ancestors.",
        ),
    ],
    output: "Without --out: exact owner response. With --out: {schema:'ds.reference-read-file/v1',path,sha256,size_bytes} for the exact serialized response bytes.",
    examples: &[],
    refusals: &[INVALID, UNAVAILABLE],
    reference: Some("docs/reference/data.md"),
    search: &["global", "rwanda", "cache", "offline"],
    requires: Requires::Server,
    availability: super::available,
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ResourceIdentity {
    resource_id: String,
    #[serde(
        default,
        deserialize_with = "present_optional",
        skip_serializing_if = "Option::is_none"
    )]
    version: Option<String>,
}

// Omission is optional; an explicitly supplied null is not an identity or limit.
fn present_optional<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    StorageStatus {},
    Status {
        identity: Identity,
    },
    Versions {
        identity: ResourceIdentity,
    },
    Query {
        identity: Identity,
        area: Value,
        #[serde(
            default,
            deserialize_with = "present_optional",
            skip_serializing_if = "Option::is_none"
        )]
        limit: Option<u64>,
    },
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid(INVALID.code, message).remedy(INVALID.remedy)
}
fn unavailable(message: impl Into<String>) -> Failure {
    Failure::unavailable(UNAVAILABLE.code, message).remedy(UNAVAILABLE.remedy)
}
fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn ordinary_ancestors(path: &Path) -> Result<(), Failure> {
    for ancestor in path.parent().into_iter().flat_map(Path::ancestors) {
        let metadata = fs::symlink_metadata(ancestor)
            .map_err(|error| invalid(format!("ordinary parent directory required: {error}")))?;
        if linked(&metadata) || !metadata.is_dir() {
            return Err(invalid("linked or non-directory ancestors are not allowed"));
        }
    }
    Ok(())
}

fn absolute(path: &Path) -> Result<PathBuf, Failure> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|error| invalid(error.to_string()))
    }
}

fn read_request(path: &Path) -> Result<Value, Failure> {
    let path = absolute(path)?;
    ordinary_ancestors(&path)?;
    let metadata =
        fs::symlink_metadata(&path).map_err(|error| invalid(format!("request file: {error}")))?;
    if linked(&metadata) || !metadata.is_file() || metadata.len() > MAX_REQUEST_BYTES {
        return Err(invalid(
            "request must be an ordinary JSON file of at most 16 MiB",
        ));
    }
    let mut bytes = Vec::new();
    open_request(&path)?
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| invalid(format!("request file: {error}")))?;
    if bytes.len() as u64 > MAX_REQUEST_BYTES {
        return Err(invalid("request exceeds 16 MiB"));
    }
    decode(&bytes)
}

fn open_request(path: &Path) -> Result<fs::File, Failure> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000).share_mode(1);
    }
    let file = options
        .open(path)
        .map_err(|error| invalid(format!("request file: {error}")))?;
    let opened = file
        .metadata()
        .map_err(|error| invalid(format!("opened request: {error}")))?;
    if linked(&opened) || !opened.is_file() || opened.len() > MAX_REQUEST_BYTES {
        return Err(invalid(
            "opened request is not an ordinary file of at most 16 MiB",
        ));
    }
    Ok(file)
}

fn decode(bytes: &[u8]) -> Result<Value, Failure> {
    let request: Request = serde_json::from_slice(bytes)
        .map_err(|error| invalid(format!("closed reference read request: {error}")))?;
    match &request {
        Request::StorageStatus {} => {}
        Request::Status { identity } | Request::Query { identity, .. } => {
            identity.validate().map_err(invalid)?
        }
        Request::Versions { identity } => Identity {
            resource_id: identity.resource_id.clone(),
            version: identity.version.clone().unwrap_or_else(|| "0".repeat(64)),
        }
        .validate()
        .map_err(invalid)?,
    }
    if let Request::Query { area, limit, .. } = &request {
        policy::validate_area(area).map_err(invalid)?;
        if limit.is_some_and(|limit| !(1..=10000).contains(&limit)) {
            return Err(invalid(
                "query limit must be between 1 and 10000; omit it for 5000",
            ));
        }
    }
    serde_json::to_value(request).map_err(|error| invalid(error.to_string()))
}

fn output_path(path: &Path) -> Result<PathBuf, Failure> {
    if !path.is_absolute() {
        return Err(invalid("--out must be an absolute new file path"));
    }
    ordinary_ancestors(path)?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Ok(_) => Err(invalid("--out already exists; choose a new file")),
        Err(error) => Err(invalid(format!("--out: {error}"))),
    }
}

fn read_with(
    inputs: &Inputs,
    owner: impl FnOnce(Value) -> Result<Value, String>,
) -> Result<Value, Failure> {
    let request = read_request(Path::new(inputs.require("request")?))?;
    let out = inputs
        .value("out")
        .map(|path| output_path(Path::new(path)))
        .transpose()?;
    let result = owner(request).map_err(unavailable)?;
    let Some(path) = out else {
        return Ok(result);
    };
    let bytes = serde_json::to_vec(&result).map_err(|error| unavailable(error.to_string()))?;
    if bytes.len() > MAX_RESULT_BYTES {
        return Err(unavailable(
            "reference result exceeds the 64 MiB file bound",
        ));
    }
    ordinary_ancestors(&path)?;
    let mut file = ds_layer_store::private::open_options()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| invalid(format!("new --out file: {error}")))?;
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err(unavailable(format!("write reference result: {error}")));
    }
    Ok(json!({"schema":"ds.reference-read-file/v1","path":path,
        "sha256":ds_command_kernel::report_export::sha256_hex(&bytes),"size_bytes":bytes.len()}))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    read_with(inputs, ds_layer_store::reference_cache::execute)
}
pub fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "ds-cli-reference-read-{}-{}",
                std::process::id(),
                ds_command_kernel::report_export::sha256_hex(
                    format!("{:?}", std::time::SystemTime::now()).as_bytes()
                )
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn identity() -> Value {
        json!({"resource_id":"a".repeat(64),"version":"b".repeat(64)})
    }
    fn area() -> Value {
        json!({"type":"Polygon","coordinates":[[[29.99,-2.01],[30.01,-2.01],[30.01,-1.99],[29.99,-1.99],[29.99,-2.01]]]})
    }
    fn inputs(path: &Path, out: Option<&Path>) -> Inputs {
        let mut tokens = vec!["--request".into(), path.to_str().unwrap().into()];
        if let Some(out) = out {
            tokens.extend(["--out".into(), out.to_str().unwrap().into()]);
        }
        ds_cli_contract::args::parse(&COMMAND, &tokens).unwrap()
    }
    fn context() -> Context {
        Context {
            confirmed: false,
            output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
        }
    }
    fn file(root: &Path, request: &Value) -> PathBuf {
        let path = root.join("request.json");
        fs::write(&path, serde_json::to_vec(request).unwrap()).unwrap();
        path
    }

    #[test]
    fn closed_read_requests_reject_write_unknown_extra_and_invalid_fields_before_storage() {
        let root = Scratch::new();
        let owner_calls = Cell::new(0);
        let mut cases = vec![
            json!({"action":"store","identity":identity(),"page":{}}),
            json!({"action":"remove","identity":identity()}),
            json!({"action":"remove_all_versions","identity":identity()}),
            json!({"action":"set_storage_root","path":"/other-root"}),
            json!({"action":"unknown"}),
            json!({"action":"storage_status","identity":identity()}),
            json!({"action":"storage_status","root":"/tmp/other"}),
            json!({"action":"status","identity":identity(),"root":"/other-root"}),
            json!({"action":"status","identity":{"resource_id":"a".repeat(64),"version":"b".repeat(64),"other":true}}),
            json!({"action":"status","identity":{"resource_id":"not-a-digest","version":"b".repeat(64)}}),
            json!({"action":"versions","identity":{"resource_id":"a".repeat(64),"version":null}}),
            json!({"action":"versions","identity":{"resource_id":"a".repeat(64),"version":"invalid"}}),
            json!({"action":"versions","identity":{"resource_id":"a".repeat(64),"other":true}}),
            json!({"action":"versions","identity":{"resource_id":"A".repeat(64)}}),
            json!({"action":"query","identity":identity(),"area":{"type":"Point","coordinates":[30.,-2.]}}),
        ];
        for limit in [
            Value::Null,
            json!(0),
            json!(10001),
            json!(1.5),
            json!(-1),
            json!("10"),
        ] {
            cases.push(json!({"action":"query","identity":identity(),"area":area(),"limit":limit}));
        }
        for request in cases {
            let path = file(&root.0, &request);
            let failure = read_with(&inputs(&path, None), |_| {
                owner_calls.set(owner_calls.get() + 1);
                Ok(Value::Null)
            })
            .unwrap_err();
            assert_eq!(failure.code(), INVALID.code, "{request}");
        }
        assert_eq!(owner_calls.get(), 0);
        let path = root.0.join("oversized.json");
        fs::File::create(&path)
            .unwrap()
            .set_len(MAX_REQUEST_BYTES + 1)
            .unwrap();
        assert_eq!(read_request(&path).unwrap_err().code(), INVALID.code);
        assert_eq!(COMMAND.authority, Authority::None);
        assert_eq!(COMMAND.effect, Effect::LocalFileWrite);
        assert!(!COMMAND.effect.needs_confirmation());
    }

    #[test]
    fn forwards_only_the_validated_read_and_preserves_owner_values_and_errors() {
        let root = Scratch::new();
        for limit in [None, Some(1), Some(10000)] {
            let mut request = json!({"action":"query","identity":identity(),"area":area()});
            if let Some(limit) = limit {
                request["limit"] = json!(limit);
            }
            let path = file(&root.0, &request);
            let result =
                json!({"features":[],"truncated":false,"owner_fact":{"unknown":null},"count":123});
            let response = read_with(&inputs(&path, None), |received| {
                assert_eq!(received, request);
                Ok(result.clone())
            })
            .unwrap();
            assert_eq!(response, result);
        }
        for identity in [identity(), json!({"resource_id":"a".repeat(64)})] {
            let request = json!({"action":"versions","identity":identity});
            let path = file(&root.0, &request);
            read_with(&inputs(&path, None), |received| {
                assert_eq!(received, request);
                Ok(json!({"versions":[]}))
            })
            .unwrap();
        }
        let path = file(&root.0, &json!({"action":"storage_status"}));
        let failure =
            read_with(&inputs(&path, None), |_| Err("fixture unavailable".into())).unwrap_err();
        assert_eq!(failure.code(), UNAVAILABLE.code);
        assert!(failure.message().contains("fixture unavailable"));
    }

    #[test]
    fn result_file_is_exact_bounded_json_with_verified_digest_and_no_clobber() {
        let root = Scratch::new();
        let path = file(&root.0, &json!({"action":"storage_status"}));
        let out = root.0.join("result.json");
        let result = json!({"retained":"école","values":[0,null,true],"features":[]});
        let receipt = read_with(&inputs(&path, Some(&out)), |_| Ok(result.clone())).unwrap();
        let bytes = fs::read(&out).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let metadata = fs::metadata(&out).unwrap();
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
            assert_eq!(metadata.nlink(), 1);
        }
        assert_eq!(bytes, serde_json::to_vec(&result).unwrap());
        assert_eq!(
            receipt,
            json!({"schema":"ds.reference-read-file/v1","path":out,
            "sha256":ds_command_kernel::report_export::sha256_hex(&bytes),"size_bytes":bytes.len()})
        );
        let failure = read_with(&inputs(&path, Some(&out)), |_| {
            panic!("existing output must refuse before owner")
        })
        .unwrap_err();
        assert_eq!(failure.code(), INVALID.code);
        assert_eq!(fs::read(&out).unwrap(), bytes);
        assert!(output_path(Path::new("relative-result.json")).is_err());
        let oversized = root.0.join("oversized-result.json");
        let failure = read_with(&inputs(&path, Some(&oversized)), |_| {
            Ok(Value::String("x".repeat(MAX_RESULT_BYTES)))
        })
        .unwrap_err();
        assert_eq!(failure.code(), UNAVAILABLE.code);
        assert!(!oversized.exists());
    }

    #[cfg(unix)]
    #[test]
    fn linked_request_and_output_ancestors_are_refused() {
        use std::os::unix::fs::symlink;
        let root = Scratch::new();
        let path = file(&root.0, &json!({"action":"storage_status"}));
        let link = root.0.join("request-link.json");
        symlink(&path, &link).unwrap();
        assert!(read_request(&link).is_err());
        assert!(
            open_request(&link).is_err(),
            "no-follow also holds at the open boundary"
        );
        assert!(open_request(&root.0).is_err(), "opened directories refuse");
        {
            use std::os::fd::AsRawFd;
            let opened = open_request(&path).unwrap();
            let flags = unsafe { libc::fcntl(opened.as_raw_fd(), libc::F_GETFL) };
            assert!(flags >= 0);
            assert_ne!(
                flags & libc::O_NONBLOCK,
                0,
                "request open cannot block on a replaced FIFO"
            );
        }
        let linked_dir = root.0.join("linked");
        symlink(&root.0, &linked_dir).unwrap();
        assert!(output_path(&linked_dir.join("result.json")).is_err());
        assert!(read_request(&linked_dir.join("request.json")).is_err());
    }

    #[test]
    fn actual_reference_owner_reads_temp_store_status_versions_and_spatial_query_offline() {
        const TEST_ROOT: &str = "DS_CLI_REFERENCE_READ_TEST_ROOT";
        let Some(root) = std::env::var_os(TEST_ROOT) else {
            let scratch = Scratch::new();
            let output=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","reference::tests::actual_reference_owner_reads_temp_store_status_versions_and_spatial_query_offline","--nocapture"])
                .env(TEST_ROOT,&scratch.0).env("XDG_DATA_HOME",&scratch.0).env("LOCALAPPDATA",&scratch.0).env("HOME",&scratch.0)
                .output().unwrap();
            assert!(
                output.status.success(),
                "isolated cache test failed: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        };
        let root = PathBuf::from(root);
        let store = ds_layer_store::reference_cache::configured_root().unwrap();
        assert!(
            store.starts_with(&root),
            "test never opens installed user data"
        );
        let feature = json!({"type":"Feature","id":"fixture","properties":{"name":"école"},"geometry":{"type":"Point","coordinates":[30.,-2.]}});
        let payload = serde_json::to_string(&vec![feature.clone()]).unwrap();
        let page = json!({"resource_id":"a".repeat(64),"version":"b".repeat(64),"offset":0,"area":null,
            "feature_json":payload,"feature_count":1,"sha256":ds_command_kernel::report_export::sha256_hex(payload.as_bytes()),"next_offset":null});
        ds_layer_store::reference_cache::execute(
            json!({"action":"store","identity":identity(),"offset":0,"page":page}),
        )
        .unwrap();
        for request in [
            json!({"action":"storage_status"}),
            json!({"action":"status","identity":identity()}),
            json!({"action":"versions","identity":{"resource_id":"a".repeat(64)}}),
            json!({"action":"versions","identity":identity()}),
            json!({"action":"query","identity":identity(),"area":area()}),
        ] {
            let path = file(&root, &request);
            let response = run(&inputs(&path, None), &context()).unwrap();
            match request["action"].as_str().unwrap() {
                "storage_status" => assert_eq!(response["root"], json!(store)),
                "status" => {
                    assert_eq!(response["ready"], true);
                    assert_eq!(response["features"], 1);
                }
                "versions" => assert_eq!(response["versions"], json!(["b".repeat(64)])),
                "query" => {
                    assert_eq!(response["features"], json!([feature]));
                    assert_eq!(response["truncated"], false);
                }
                _ => unreachable!(),
            }
        }
    }
}
