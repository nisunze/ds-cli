//! Bounded file transport for the existing closed Solar application receipt.
use ds_cli_contract::{Failure, Inputs};
use ds_solar_native::application::{MAX_FILE_RESPONSE_BYTES, Request};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

const MAX_REQUEST: usize = 32 * 1024 * 1024;
const MAX_COMPLETION: usize = 64 * 1024;
const FILE_SCHEMA: &str = "ds-solar.application-artifact/v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt<'a> {
    schema: &'a str,
    project_id: &'a str,
    root: &'a str,
    #[serde(rename = "value")]
    _value: serde::de::IgnoredAny,
}

pub(crate) fn command(inputs: &Inputs) -> Result<Value, Failure> {
    command_with(inputs, &|project, bytes, limit, timeout| {
        crate::request_with_timeout(
            inputs,
            "POST",
            &crate::with_project("/v1/solar-application", project),
            Some(bytes),
            limit as u64,
            timeout,
        )
    })
}

fn command_with(
    inputs: &Inputs,
    request: &dyn Fn(&str, &[u8], usize, u64) -> Result<Vec<u8>, Failure>,
) -> Result<Value, Failure> {
    crate::require_native_host()?;
    let output = inputs.value("out").map(PathBuf::from);
    if let Some(path) = &output {
        validate_output(path)?;
    }
    let path = PathBuf::from(inputs.require("request")?);
    let metadata = std::fs::symlink_metadata(&path).map_err(crate::failure)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_REQUEST as u64
    {
        return Err(crate::failure(
            "Solar application request must be one ordinary file no larger than 32 MiB",
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(crate::failure)?
        .take(MAX_REQUEST as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(crate::failure)?;
    if bytes.len() > MAX_REQUEST {
        return Err(crate::failure("Solar application request exceeds 32 MiB"));
    }
    let intent = serde_json::from_slice::<Request>(&bytes)
        .map_err(|_| crate::failure("Invalid closed native Solar application request"))?;
    #[derive(Deserialize)]
    struct Operation {
        operation: String,
    }
    let action: Operation = serde_json::from_slice(&bytes).map_err(crate::failure)?;
    let project = crate::project(inputs)?;
    let lane = inputs.require("lane")?;
    let limit = if output.is_some() {
        MAX_FILE_RESPONSE_BYTES
    } else {
        MAX_REQUEST
    };
    let response = request(&project, &bytes, limit, intent.response_timeout_seconds())?;
    if response.len() > limit {
        return Err(crate::failure(
            "Core returned an oversized Solar application receipt",
        ));
    }
    match output {
        Some(path) => {
            // IgnoredAny checks complete JSON without allocating a Value per
            // byte of an admitted 64 MiB artifact's integer array.
            let receipt: Receipt<'_> = serde_json::from_slice(&response).map_err(|_| {
                crate::failure("Core returned an invalid Solar application receipt")
            })?;
            if receipt.schema != "ds-solar.application-receipt/v1"
                || receipt.project_id != project
                || receipt.root != format!("eds_project/{project}/eds_solar")
            {
                return Err(crate::failure(
                    "Core returned a Solar application receipt for another project or root",
                ));
            }
            finish_output(
                &path,
                &response,
                &project,
                receipt.root,
                lane,
                &action.operation,
            )
        }
        None => {
            let value: Value = serde_json::from_slice(&response).map_err(|_| {
                crate::failure("Core returned an invalid Solar application receipt")
            })?;
            if value["schema"] != "ds-solar.application-receipt/v1"
                || value["project_id"] != project
            {
                return Err(crate::failure(
                    "Core returned a Solar application receipt for another project",
                ));
            }
            Ok(value)
        }
    }
}

fn validate_output(path: &Path) -> Result<(), Failure> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(crate::failure("Solar output must be a new absolute file"));
    }
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        _ => {
            return Err(crate::failure(
                "Solar output must be a new absolute file; existing files are never replaced",
            ));
        }
    }
    let parent = path
        .parent()
        .ok_or_else(|| crate::failure("Solar output has no parent"))?;
    ds_solar_native::workspace::secure_existing_private_directory(parent)
        .map_err(crate::failure)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(parent).map_err(crate::failure)?;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(crate::failure(
                "Solar output parent must be private to the current OS account",
            ));
        }
    }
    #[cfg(windows)]
    ds_cli_auth::windows_host::validate_directory(parent).map_err(crate::failure)?;
    Ok(())
}

fn finish_output(
    path: &Path,
    bytes: &[u8],
    project: &str,
    root: &str,
    lane: &str,
    action: &str,
) -> Result<Value, Failure> {
    let digest = ds_compute_runtime::digest(bytes);
    write_output(path, bytes, &digest).map_err(|cause| {
        let completion = if bytes.len() <= MAX_COMPLETION {
            serde_json::from_slice::<Value>(bytes).ok().and_then(|mut value| value.get_mut("value").map(Value::take))
        } else { None };
        Failure::failed("server_refused", "Solar action completed, but its result file could not be delivered")
            .remedy("Inspect the completed operation and scoped completion details before retrying; preserve committed work and do not repeat an uncertain effect")
            .detail(json!({"completed":true,"completed_operation":action,"project_id":project,"root":root,"lane":lane,"byte_count":bytes.len(),"sha256":digest,"completion":completion,"output_error":cause.message()}))
    })?;
    Ok(
        json!({"schema":FILE_SCHEMA,"out":path,"byte_count":bytes.len(),"sha256":digest,"project_id":project,"root":root,"lane":lane}),
    )
}

fn write_output(path: &Path, bytes: &[u8], digest: &str) -> Result<(), Failure> {
    validate_output(path)?;
    if bytes.len() > MAX_FILE_RESPONSE_BYTES {
        return Err(crate::failure("Solar output exceeds 257 MiB"));
    }
    // Existing private temporary-file + persist_noclobber owner, as Profile
    // uses. Never unlink a caller's existing or subsequently replaced file.
    ds_design_workspace::write_new(path, bytes).map_err(crate::failure)?;
    #[cfg(windows)]
    let mut file = ds_cli_auth::windows_host::read_file(path).map_err(crate::failure)?;
    #[cfg(not(windows))]
    let mut file = {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        options.open(path).map_err(crate::failure)?
    };
    let metadata = file.metadata().map_err(crate::failure)?;
    if !metadata.is_file() || metadata.len() != bytes.len() as u64 {
        return Err(crate::failure("Solar output changed after writing"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != 0o600
            || metadata.nlink() != 1
        {
            return Err(crate::failure(
                "Solar output is not a private ordinary single-link file",
            ));
        }
    }
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut count = 0usize;
    loop {
        let read = file.read(&mut buffer).map_err(crate::failure)?;
        if read == 0 {
            break;
        }
        count += read;
        if count > bytes.len() {
            return Err(crate::failure("Solar output grew after writing"));
        }
        hash.update(&buffer[..read]);
    }
    if count != bytes.len() || format!("{:x}", hash.finalize()) != digest {
        return Err(crate::failure("Solar output digest changed after writing"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::spec::Arg;
    fn directory() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        dir
    }
    fn inputs(request: &Path, out: Option<&Path>) -> Inputs {
        const ARGS: &[Arg] = &[
            Arg::value("project", "<id>", "project").required(),
            Arg::value("lane", "<lane>", "lane").required(),
            Arg::value("request", "<file>", "request").required(),
            Arg::value("out", "<file>", "output"),
        ];
        let command = ds_cli_contract::spec::Command {
            args: ARGS,
            ..crate::STATUS
        };
        let mut args = vec![
            "--project".into(),
            "fixture-project".into(),
            "--lane".into(),
            "canary".into(),
            "--request".into(),
            request.display().to_string(),
        ];
        if let Some(out) = out {
            args.extend(["--out".into(), out.display().to_string()]);
        }
        ds_cli_contract::args::parse(&command, &args).unwrap()
    }
    fn request(dir: &Path) -> PathBuf {
        let path = dir.join("request.json");
        std::fs::write(&path, br#"{"operation":"workspace_selection_read"}"#).unwrap();
        path
    }
    fn receipt(value: Value) -> Vec<u8> {
        serde_json::to_vec(&json!({"schema":"ds-solar.application-receipt/v1","project_id":"fixture-project","root":"eds_project/fixture-project/eds_solar","value":value})).unwrap()
    }
    #[test]
    fn large_exact_receipt_uses_small_scope_bound_private_file() {
        let dir = directory();
        let req = request(dir.path());
        let out = dir.path().join("result.json");
        let bytes = receipt(json!({"bytes":vec![255u8; 2*1024*1024]}));
        assert!(bytes.len() > 4 * 1024 * 1024);
        let result = command_with(&inputs(&req, Some(&out)), &|project, _, limit, _| {
            assert_eq!(project, "fixture-project");
            assert_eq!(limit, MAX_FILE_RESPONSE_BYTES);
            Ok(bytes.clone())
        })
        .unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), bytes);
        assert_eq!(result["sha256"], ds_compute_runtime::digest(&bytes));
        assert_eq!(result["byte_count"], bytes.len());
        assert_eq!(result["schema"], FILE_SCHEMA);
        assert_eq!(result["lane"], "canary");
        assert_eq!(result["root"], "eds_project/fixture-project/eds_solar");
        assert!(serde_json::to_vec(&result).unwrap().len() < 4096);
    }
    #[test]
    fn existing_destination_refuses_before_request_or_effect() {
        let dir = directory();
        let out = dir.path().join("occupied.json");
        std::fs::write(&out, b"keep").unwrap();
        assert!(
            command_with(
                &inputs(&dir.path().join("absent-request"), Some(&out)),
                &|_, _, _, _| panic!("must not call Core")
            )
            .unwrap_err()
            .message()
            .contains("existing files")
        );
        assert_eq!(std::fs::read(&out).unwrap(), b"keep");
    }
    #[test]
    fn core_refusal_is_unchanged_and_creates_no_output() {
        let dir = directory();
        let req = request(dir.path());
        let out = dir.path().join("result.json");
        let error = command_with(&inputs(&req, Some(&out)), &|_, _, _, _| {
            Err(
                Failure::unauthorized("server_owner_changed", "owner changed")
                    .remedy("sign in")
                    .detail(json!({"test":"kept"})),
            )
        })
        .unwrap_err();
        assert_eq!(error.code(), "server_owner_changed");
        assert_eq!(error.detail_value().unwrap()["test"], "kept");
        assert!(!out.exists());
    }
    #[test]
    fn no_out_retains_json_and_original_response_bound() {
        let dir = directory();
        let req = request(dir.path());
        let bytes = receipt(json!({"run_id":"run-one"}));
        let result = command_with(&inputs(&req, None), &|_, _, limit, _| {
            assert_eq!(limit, 32 * 1024 * 1024);
            Ok(bytes.clone())
        })
        .unwrap();
        assert_eq!(result, serde_json::from_slice::<Value>(&bytes).unwrap());
    }
    #[test]
    fn valid_escaped_operation_retains_existing_request_semantics() {
        let dir = directory();
        let req = request(dir.path());
        std::fs::write(&req, br#"{"operation":"workspace_selection_re\u0061d"}"#).unwrap();
        let bytes = receipt(Value::Null);
        let result = command_with(&inputs(&req, None), &|_, _, _, _| Ok(bytes.clone())).unwrap();
        assert_eq!(result["value"], Value::Null);
    }
    #[test]
    fn crossed_receipt_refuses_without_writing() {
        let dir = directory();
        let req = request(dir.path());
        for field in ["schema", "project_id", "root"] {
            let out = dir.path().join(format!("{field}.json"));
            let mut value: Value = serde_json::from_slice(&receipt(Value::Null)).unwrap();
            value[field] = json!("foreign");
            assert!(
                command_with(&inputs(&req, Some(&out)), &|_, _, _, _| Ok(
                    serde_json::to_vec(&value).unwrap()
                ))
                .is_err()
            );
            assert!(!out.exists());
        }
    }
    #[test]
    fn late_occupied_output_preserves_completed_launch_and_existing_bytes() {
        let dir = directory();
        let req = request(dir.path());
        let out = dir.path().join("late.json");
        let bytes = receipt(json!({"run_id":"started-run","status":"running"}));
        let error = command_with(&inputs(&req, Some(&out)), &|_, _, _, _| {
            std::fs::write(&out, b"other-owner").unwrap();
            Ok(bytes.clone())
        })
        .unwrap_err();
        assert_eq!(error.code(), "server_refused");
        let detail = error.detail_value().unwrap();
        assert_eq!(detail["completed"], true);
        assert_eq!(detail["completion"]["run_id"], "started-run");
        assert_eq!(detail["completed_operation"], "workspace_selection_read");
        assert_eq!(detail["sha256"], ds_compute_runtime::digest(&bytes));
        assert_eq!(std::fs::read(&out).unwrap(), b"other-owner");
    }
    #[test]
    fn relative_linked_and_public_output_parents_refuse() {
        assert!(validate_output(Path::new("relative.json")).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::{PermissionsExt, symlink};
            let dir = directory();
            let linked = dir.path().join("linked");
            symlink(dir.path().join("missing"), &linked).unwrap();
            assert!(validate_output(&linked).is_err());
            let alias = dir.path().join("alias");
            symlink(dir.path(), &alias).unwrap();
            assert!(validate_output(&alias.join("result.json")).is_err());
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(validate_output(&dir.path().join("result.json")).is_err());
        }
    }
}
