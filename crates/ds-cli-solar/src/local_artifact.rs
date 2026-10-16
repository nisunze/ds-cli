//! Bounded local artifact IO; validation remains with the pure Solar owner.
use ds_cli_contract::Failure;
use serde::de::DeserializeOwned;
use serde_json::json;
use std::{fs::File, io::Read, path::Path};

pub(crate) const MAX_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) fn refused(operation: &str, error: impl std::fmt::Display) -> Failure {
    let message = error
        .to_string()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(6)
        .map(|line| line.chars().take(200).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    Failure::failed("engine_refused", format!("Solar {operation} validation did not complete"))
        .remedy("read detail.engine; retain the original artifact and regenerate changed data through its Solar owner")
        .detail(json!({"owner":"ds-solar-contracts","operation":operation,"engine":message}))
}

pub(crate) fn read<T: DeserializeOwned>(path: &Path, operation: &str) -> Result<T, Failure> {
    let fail = |error: std::io::Error| refused(operation, error);
    let file = File::open(path).map_err(fail)?;
    let metadata = file.metadata().map_err(fail)?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err(refused(
            operation,
            "Solar artifact must be an ordinary file no larger than 64 MiB",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len().min(64 * 1024) as usize);
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(fail)?;
    if bytes.len() as u64 > MAX_BYTES || bytes.len() as u64 != metadata.len() {
        return Err(refused(
            operation,
            "Solar artifact exceeded 64 MiB or changed size while it was read",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|error| refused(operation, error))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_invalid_and_non_file_artifacts_refuse() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("artifact.json");
        let file = File::create(&path).unwrap();
        file.set_len(MAX_BYTES + 1).unwrap();
        assert_eq!(
            read::<serde_json::Value>(&path, "test").unwrap_err().code(),
            "engine_refused"
        );
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(
            read::<serde_json::Value>(&path, "test").unwrap_err().code(),
            "engine_refused"
        );
        assert_eq!(
            read::<serde_json::Value>(dir.path(), "test")
                .unwrap_err()
                .code(),
            "engine_refused"
        );
    }
    #[test]
    fn refusal_detail_is_bounded_without_inventing_a_child_process() {
        let error = refused("compare", "x".repeat(5000));
        let detail = error.detail_value().unwrap();
        assert_eq!(detail["engine"].as_str().unwrap().len(), 200);
        assert_eq!(detail["owner"], "ds-solar-contracts");
        assert!(detail.get("exit_code").is_none());
        assert!(detail.get("binary").is_none());
    }
}
