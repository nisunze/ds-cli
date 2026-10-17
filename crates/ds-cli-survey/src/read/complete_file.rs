//! Complete original held-feature delivery; the ordinary bounded reader is unchanged.
use super::*;
use ds_client_core::survey_entries_read::SURVEY_ENTRIES_READ_RESPONSE_LIMIT;
use sha2::{Digest, Sha256};

pub(super) fn admit(inputs: &Inputs) -> Result<(), Failure> {
    if inputs.switch("complete-file")
        && (inputs.value("out").is_none() || inputs.switch("include-deleted"))
    {
        return Err(invalid(
            "--complete-file requires --out and the admitted held view; --include-deleted is a separate bounded cloud read",
        ));
    }
    Ok(())
}
fn document(read: &SurveyEntriesRead, features: Vec<Value>) -> Value {
    json!({"type":"FeatureCollection","ds_project":read.project,"form":read.form,"total":read.total,"truncated":false,"features":features})
}
fn output(error: impl std::fmt::Display) -> Failure {
    Failure::invalid("survey_entries_output", error.to_string())
        .remedy("Choose a fresh writable output file; retain the original held form")
}
pub(super) fn write(path: &str, read: &SurveyEntriesRead) -> Result<Value, Failure> {
    if read.truncated || read.entries.len() as u64 != read.total {
        return Err(output(
            "complete held-feature delivery cannot contain a truncated view",
        ));
    }
    // Each original feature is the held NDJSON row: serialization plus one
    // newline. Reuse its existing transport bound, not a new global file cap.
    let mut source_bytes = 0usize;
    let mut features = Vec::with_capacity(read.entries.len());
    for entry in &read.entries {
        let size = serde_json::to_vec(&entry.feature).map_err(output)?.len();
        source_bytes = source_bytes
            .checked_add(size + 1)
            .ok_or_else(|| output("held-view byte count overflow"))?;
        if source_bytes > SURVEY_ENTRIES_READ_RESPONSE_LIMIT {
            return Err(output(
                "complete held view exceeds the owning survey source-format response bound",
            ));
        }
        features.push(entry.feature.clone());
    }
    let header_bytes = serde_json::to_vec(&document(read, Vec::new()))
        .map_err(output)?
        .len();
    let encoded = serde_json::to_vec(&document(read, features)).map_err(output)?;
    let limit = SURVEY_ENTRIES_READ_RESPONSE_LIMIT
        .checked_add(header_bytes)
        .ok_or_else(|| output("file bound overflow"))?;
    if encoded.len() > limit {
        return Err(output(
            "complete GeoJSON exceeds source-format plus exact header bound",
        ));
    }
    #[cfg(windows)]
    let mut file =
        ds_cli_auth::windows_host::create_private_output_file(std::path::Path::new(path))
            .map_err(output)?;
    #[cfg(not(windows))]
    let mut file = {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path).map_err(output)?
    };
    if let Err(error) = file.write_all(&encoded).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(output(error));
    }
    Ok(
        json!({"schema":"ds.survey-held-file/v1","path":path,"project":read.project,"form":read.form,"feature_count":read.total,"size_bytes":encoded.len(),"sha256":format!("{:x}",Sha256::digest(&encoded)),"source_limit_bytes":SURVEY_ENTRIES_READ_RESPONSE_LIMIT,"source_bytes":source_bytes,"header_bytes":header_bytes}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(count: usize) -> SurveyEntriesRead {
        SurveyEntriesRead {project:"p1".into(),form:"customers".into(),total:count as u64,truncated:false,entries:(0..count).map(|i|SurveyEntry{feature:json!({"type":"Feature","id":i,"geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"exact":"held \n original","index":i}}),media:Vec::new()}).collect(),sync:None}
    }
    fn root() -> std::path::PathBuf {
        #[cfg(windows)]
        let base = ds_cli_auth::windows_host::local_application_data().unwrap();
        #[cfg(not(windows))]
        let base = std::env::temp_dir();
        let p = base.join(format!(
            "ds-complete-survey-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&p).unwrap();
        p
    }
    #[test]
    fn complete_file_preserves_more_than_five_thousand_original_features_and_identity() {
        let dir = root();
        let path = dir.join("complete.geojson");
        let read = fixture(6001);
        let receipt = write(path.to_str().unwrap(), &read).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let bytes = std::fs::read(&path).unwrap();
        let doc: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(doc["features"].as_array().unwrap().len(), 6001);
        assert_eq!(doc["features"][6000], read.entries[6000].feature);
        assert_eq!(doc["truncated"], false);
        assert_eq!(receipt["feature_count"], 6001);
        assert_eq!(receipt["size_bytes"], bytes.len());
        assert_eq!(receipt["sha256"], format!("{:x}", Sha256::digest(&bytes)));
        assert!(
            bytes.len()
                <= SURVEY_ENTRIES_READ_RESPONSE_LIMIT
                    + receipt["header_bytes"].as_u64().unwrap() as usize
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn complete_file_is_readable_by_private_output_consumer_while_inherited_file_is_refused() {
        use std::io::Read;

        let dir = root();
        let grant = std::process::Command::new("icacls.exe")
            .arg(&dir)
            .args(["/grant", "*S-1-1-0:(OI)(CI)(R)"])
            .output()
            .unwrap();
        assert!(
            grant.status.success(),
            "native inherited ACL fixture failed"
        );
        let path = dir.join("complete.geojson");
        let read = fixture(2);
        let receipt = write(path.to_str().unwrap(), &read).unwrap();
        let mut bytes = Vec::new();
        ds_cli_auth::windows_host::read_private_output_file(&path)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let doc: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(doc["features"][1], read.entries[1].feature);
        assert_eq!(receipt["size_bytes"], bytes.len());
        assert_eq!(receipt["sha256"], format!("{:x}", Sha256::digest(&bytes)));

        let inherited = dir.join("inherited.geojson");
        std::fs::write(&inherited, &bytes).unwrap();
        assert!(ds_cli_auth::windows_host::read_private_output_file(&inherited).is_err());
        assert_eq!(std::fs::read(&inherited).unwrap(), bytes);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn never_clobbers_existing_output_or_delivers_a_partial_view() {
        let dir = root();
        let path = dir.join("held.geojson");
        std::fs::write(&path, b"operator bytes").unwrap();
        assert!(write(path.to_str().unwrap(), &fixture(1)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"operator bytes");
        let mut partial = fixture(1);
        partial.total = 2;
        partial.truncated = true;
        assert!(write(dir.join("partial.geojson").to_str().unwrap(), &partial).is_err());
        assert!(!dir.join("partial.geojson").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn ordinary_output_stays_bounded_while_complete_output_refuses_partial_data() {
        let dir = root();
        let path = dir.join("ordinary.geojson");
        let mut read = fixture(6001);
        read.entries.truncate(5000);
        read.truncated = true;
        super::super::write_new(path.to_str().unwrap(), &read).unwrap();
        let doc: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(doc["features"].as_array().unwrap().len(), 5000);
        assert_eq!(doc["total"], 6001);
        assert_eq!(doc["truncated"], true);
        assert!(write(dir.join("complete.geojson").to_str().unwrap(), &read).is_err());
        assert!(!dir.join("complete.geojson").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn owner_source_format_limit_refuses_before_creating_an_output() {
        let dir = root();
        let path = dir.join("oversize.geojson");
        let mut read = fixture(1);
        read.entries[0].feature["properties"]["padding"] =
            json!("x".repeat(SURVEY_ENTRIES_READ_RESPONSE_LIMIT));
        assert!(write(path.to_str().unwrap(), &read).is_err());
        assert!(!path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn complete_mode_refuses_unsafe_combinations_before_authority() {
        for flags in [
            vec!["--complete-file"],
            vec![
                "--complete-file",
                "--out",
                "fresh.geojson",
                "--include-deleted",
            ],
        ] {
            let mut args = vec!["--project", "p1", "--form", "customers"];
            args.extend(flags);
            assert_eq!(
                admit(&super::super::tests::inputs(&args))
                    .unwrap_err()
                    .code(),
                "survey_entries_invalid"
            );
        }
        assert!(
            admit(&super::super::tests::inputs(&[
                "--project",
                "p1",
                "--form",
                "customers"
            ]))
            .is_ok()
        );
    }
}
