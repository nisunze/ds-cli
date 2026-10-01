//! Read saved analysis and held layers; no process, save or reporter call.
use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::path::Path;

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};
use zip::{ZipWriter, write::SimpleFileOptions};

use super::artifact::{ArtifactContract, ensure_absent, sha256, write_new};

const ARTIFACT: ArtifactContract = ArtifactContract {
    name: "saved LV analysis bundle",
    stage_tag: "ds-lv-saved-analysis",
    exists_code: "lv_analysis_output_exists",
    write_code: "lv_analysis_output_write_failed",
    exists_remedy: "Choose an absent --out path; existing files and symlinks are never replaced.",
    write_remedy: "Choose a writable absent path; retry this read without processing or saving the model.",
};

const OWN: &[Refusal] = &[
    Refusal {
        code: "lv_analysis_missing",
        when: "the saved head has no analysis receipt",
        remedy: "The model needs an authorized native analysis save; this read never computes or repairs it.",
    },
    Refusal {
        code: "lv_analysis_stale",
        when: "the saved receipt or held layer digest is stale",
        remedy: "Read the current head again; a missing current analysis requires an authorized analysis save, never a report-time recomputation.",
    },
    Refusal {
        code: "lv_analysis_bound_refused",
        when: "held layers exceed 64 MiB or saved analysis exceeds the native 32 MiB bound",
        remedy: "Use an owner-supported smaller snapshot; no layers or analysis are truncated.",
    },
    Refusal {
        code: "lv_analysis_output_exists",
        when: "--out already exists, including a symlink",
        remedy: ARTIFACT.exists_remedy,
    },
    Refusal {
        code: "lv_analysis_output_write_failed",
        when: "the bundle cannot be atomically published",
        remedy: ARTIFACT.write_remedy,
    },
];

const fn refusals() -> [Refusal; super::project_export::COMMAND.refusals.len() + OWN.len()] {
    let mut all = [OWN[0]; super::project_export::COMMAND.refusals.len() + OWN.len()];
    let mut i = 0;
    while i < super::project_export::COMMAND.refusals.len() {
        all[i] = super::project_export::COMMAND.refusals[i];
        i += 1;
    }
    let mut j = 0;
    while j < OWN.len() {
        all[i + j] = OWN[j];
        j += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "design.lv.analysis-read",
    path: &["design", "lv", "analysis-read"],
    contract: 1,
    summary: "Read current saved LV analysis JSON and its held model.",
    purpose: "Read one explicit project's persisted transformer layers and compact analysis receipt under one restored user/device session, then fetch exact saved ds.lv-voltage-drop.analysis/v1 bytes under its version, content digest and analysis SHA-256. The server refuses moved heads, stale governed nature, missing or invalid analysis. This command never computes, heals, saves, uploads or renders. Publish one atomic create-new ZIP containing saved-analysis.json (unchanged producer bytes), layers.json (the Reporter plain_local transformer_document shape, with saved layers under its layers member), and source.json (project/lane/transformer and all pins). Extract these files for existing Reporter requests: the dedicated A4 request takes the layers member and saved JSON path/SHA; the transformer A0 request takes layers.json as transformer_document and the same saved source. The bundle proves only the captured head, not continuing freshness after this call. Legacy vd_summary alone is insufficient and is never reconstructed into an analysis.",
    chapter: Chapter::Design,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        crate::LANE_ARG,
        Arg::value(
            "transformer",
            "<name>",
            "One exact existing transformer in the explicit project.",
        )
        .required(),
        Arg::value(
            "out",
            "<absent.zip>",
            "Absent local ZIP path; existing paths are never overwritten.",
        )
        .required(),
    ],
    output: "Bundle path/SHA-256/bytes, explicit project/lane/transformer, saved head version/content digest, exact analysis SHA-256/bytes, and saved:false/processed:false/published:false. Missing/stale analysis refuses with captured head pins and analysis state in detail; no partial bundle is published.",
    examples: &[Example {
        command: "ds design lv analysis-read --project <id> --transformer T-1042 --out ./T-1042.saved-analysis.zip --output json",
        note: "Read existing saved analysis without altering the model.",
        runnable: false,
    }],
    refusals: &refusals(),
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

struct Bundle {
    analysis: Vec<u8>,
    layers: BTreeMap<String, Value>,
    source: Value,
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    run_with(inputs, |lane, project, transformer| {
        let held = ds_cli_auth::saved_transformer_analysis_for_project(lane, project, transformer)?;
        let held = held.into_result();
        let snapshot = held.snapshot;
        let metadata = snapshot
            .voltage_drop_metadata()
            .cloned()
            .unwrap_or(Value::Null);
        let Some(analysis) = held.document else {
            let stale = matches!(
                metadata["state"].as_str(),
                Some("stale" | "needs_reprocess")
            );
            return Err(Failure::conflict(if stale { "lv_analysis_stale" } else { "lv_analysis_missing" },
                "The saved head has no current complete analysis; no calculation was attempted.")
                .remedy(if stale { OWN[1].remedy } else { OWN[0].remedy })
                .detail(json!({"project":project,"transformer":transformer,"lane":lane,
                    "version":snapshot.metadata().version(),"content_digest":snapshot.metadata().content_digest(),
                    "analysis_state":metadata["state"].as_str().unwrap_or("missing"),
                    "processed":false,"saved":false})));
        };
        let digest =
            ds_command_kernel::report_export::jcs::layers_content_digest(snapshot.layers())
                .map_err(|_| stale_layers())?;
        if snapshot.metadata().content_digest() != Some(digest.as_str()) {
            return Err(stale_layers());
        }
        Ok(Bundle {
            source: json!({"schema":"ds.lv-saved-analysis.source/v1", "project":project,
            "transformer":transformer,"lane":lane,"version":snapshot.metadata().version(),
            "content_digest":digest,"analysis_sha256":metadata["analysis_sha256"],
            "analysis_bytes":analysis.len(),"analysis_schema":metadata["schema"],"method":metadata["method"]}),
            analysis,
            layers: snapshot.layers().clone(),
        })
    })
}

fn stale_layers() -> Failure {
    Failure::conflict(
        "lv_analysis_stale",
        "Held saved layers do not match the captured content digest.",
    )
    .remedy(OWN[1].remedy)
}

fn run_with(
    inputs: &Inputs,
    read: impl FnOnce(&str, &str, &str) -> Result<Bundle, Failure>,
) -> Result<Value, Failure> {
    let path = Path::new(inputs.require("out")?);
    ensure_absent(path, &ARTIFACT)?;
    let bundle = read(
        inputs.require("lane")?,
        inputs.require("project")?,
        inputs.require("transformer")?,
    )?;
    let bytes = encode_bundle(&bundle)?;
    write_new(path, &bytes, &ARTIFACT)?;
    Ok(
        json!({"out":path,"bundle_sha256":sha256(&bytes),"byte_count":bytes.len(),
        "source":bundle.source,"processed":false,"saved":false,"published":false}),
    )
}

fn encode_bundle(bundle: &Bundle) -> Result<Vec<u8>, Failure> {
    let layers = serde_json::to_vec(&json!({"layers":bundle.layers})).map_err(write_failure)?;
    if bundle.analysis.len() > ds_client_core::TRANSFORMER_ANALYSIS_RESPONSE_LIMIT
        || layers.len() > 64 * 1024 * 1024
    {
        return Err(Failure::invalid(
            "lv_analysis_bound_refused",
            "Saved analysis/layers exceed the bounded bundle inputs.",
        )
        .remedy(OWN[2].remedy));
    }
    let source = serde_json::to_vec(&bundle.source).map_err(write_failure)?;
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in [
        ("saved-analysis.json", bundle.analysis.as_slice()),
        ("layers.json", layers.as_slice()),
        ("source.json", source.as_slice()),
    ] {
        archive.start_file(name, options).map_err(write_failure)?;
        archive.write_all(bytes).map_err(write_failure)?;
    }
    Ok(archive.finish().map_err(write_failure)?.into_inner())
}

fn write_failure(error: impl std::fmt::Display) -> Failure {
    Failure::failed(
        "lv_analysis_output_write_failed",
        format!("Could not stage saved analysis bundle: {error}"),
    )
    .remedy(ARTIFACT.write_remedy)
}

pub fn render(value: &Value) -> String {
    format!(
        "Saved LV analysis read for {}: {}",
        value["source"]["transformer"].as_str().unwrap_or(""),
        value["out"].as_str().unwrap_or("")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn inputs(path: &Path) -> Inputs {
        let tokens = [
            "--project",
            "p",
            "--transformer",
            "T1",
            "--out",
            path.to_str().unwrap(),
        ]
        .map(str::to_owned);
        ds_cli_contract::args::parse(&COMMAND, &tokens).unwrap()
    }

    #[test]
    fn analysis_read_bundle_preserves_exact_bytes_and_held_layers() {
        let raw = b"{\n \"schema\": \"ds.lv-voltage-drop.analysis/v1\", \"value\":1.2300 }\n";
        let layers = BTreeMap::from([(
            "tr".into(),
            json!({"type":"FeatureCollection","features":[]}),
        )]);
        let bundle = Bundle {
            analysis: raw.to_vec(),
            layers: layers.clone(),
            source: json!({"version":3}),
        };
        let bytes = encode_bundle(&bundle).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(zip.len(), 3);
        let mut actual = Vec::new();
        zip.by_name("saved-analysis.json")
            .unwrap()
            .read_to_end(&mut actual)
            .unwrap();
        assert_eq!(actual, raw);
        let decoded: Value = serde_json::from_reader(zip.by_name("layers.json").unwrap()).unwrap();
        assert_eq!(decoded, json!({"layers":layers}));
    }

    #[test]
    fn analysis_read_stale_server_refusal_publishes_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("analysis.zip");
        let error = run_with(&inputs(&path), |lane, project, name| {
            assert_eq!((lane, project, name), ("stable", "p", "T1"));
            Err(
                Failure::conflict("auth_response_unreadable", "saved head moved")
                    .detail(json!({"http_status":409,"service_code":"TRANSFORMER_ANALYSIS_STALE"})),
            )
        })
        .unwrap_err();
        assert_eq!(
            error.detail_value().unwrap()["service_code"],
            "TRANSFORMER_ANALYSIS_STALE"
        );
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    }

    #[test]
    fn analysis_read_existing_output_refuses_before_authentication() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("analysis.zip");
        std::fs::write(&path, b"owner bytes").unwrap();
        let error =
            run_with(&inputs(&path), |_, _, _| panic!("must not authenticate")).unwrap_err();
        assert_eq!(error.code(), "lv_analysis_output_exists");
        assert_eq!(std::fs::read(path).unwrap(), b"owner bytes");
    }

    #[test]
    fn analysis_read_output_race_preserves_other_writers_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("analysis.zip");
        let error = run_with(&inputs(&path), |_, _, _| {
            std::fs::write(&path, b"other writer").unwrap();
            Ok(Bundle {
                analysis: b"{}".to_vec(),
                layers: BTreeMap::new(),
                source: json!({}),
            })
        })
        .unwrap_err();
        assert_eq!(error.code(), "lv_analysis_output_exists");
        assert_eq!(std::fs::read(&path).unwrap(), b"other writer");
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn analysis_read_dangling_symlink_is_existing_owned_output() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("analysis.zip");
        std::os::unix::fs::symlink(temp.path().join("absent"), &path).unwrap();
        let error =
            run_with(&inputs(&path), |_, _, _| panic!("must not authenticate")).unwrap_err();
        assert_eq!(error.code(), "lv_analysis_output_exists");
        assert!(
            std::fs::symlink_metadata(path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
