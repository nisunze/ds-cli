//! Fixed saved LV operations. Engineering validity remains in Network.
use super::*;
use ds_network_contracts::lv_processing::*;

#[derive(Debug)]
pub struct LvPublicationOutcome {
    pub bytes: Vec<u8>,
    pub receipt: NetworkReceiptV1,
    pub result: NativeLvPublication,
}
#[derive(Debug)]
pub struct LvAnalysisValidationOutcome {
    pub bytes: Vec<u8>,
    pub receipt: NetworkReceiptV1,
    pub result: NativeLvAnalysisValidationResultV1,
}
enum SavedTask<'a> {
    Project {
        request: &'a [u8],
        result: &'a [u8],
        transformer: &'a str,
        schema: &'static str,
    },
    Validate {
        request: &'a [u8],
    },
}

impl CapturedNetworkClient {
    pub fn project_lv(
        &self,
        input: &[u8],
        result: &[u8],
        transformer: &str,
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<LvPublicationOutcome, NetworkFailure> {
        let request = decode_native_lv_request(input).map_err(NetworkFailure::native)?;
        let selected = request
            .jobs
            .iter()
            .find(|job| job.transformer_name == transformer)
            .ok_or_else(|| {
                NetworkFailure::new(
                    "network_input_invalid",
                    "selected transformer is absent from the request",
                )
            })?;
        if result.is_empty() || result.len() > MAX_NATIVE_LV_OUTPUT_BYTES {
            return Err(NetworkFailure::new(
                "network_input_invalid",
                "retained LV result is outside its byte bound",
            ));
        }
        let schema = native_lv_publication_schema_for_request(&request.schema)
            .map_err(NetworkFailure::native)?;
        let (bytes, receipt) = self.saved_lv_task(
            SavedTask::Project {
                request: input,
                result,
                transformer,
                schema,
            },
            scratch,
            cancel,
        )?;
        let publication: NativeLvPublication = serde_json::from_slice(&bytes)
            .map_err(|error| NetworkFailure::new("network_result_invalid", error.to_string()))?;
        if serde_json::to_value(&publication.process_input)
            .map_err(|error| NetworkFailure::new("network_result_invalid", error.to_string()))?
            != serde_json::to_value(selected)
                .map_err(|error| NetworkFailure::new("network_input_invalid", error.to_string()))?
            || publication.source_layers != selected.gdfs.clone().into_iter().collect()
            || publication.config_dfs != selected.config_dfs.clone().into_iter().collect()
        {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "publication source differs from the selected exact request",
            ));
        }
        Ok(LvPublicationOutcome {
            bytes,
            receipt,
            result: publication,
        })
    }

    pub fn validate_lv_analysis(
        &self,
        request: &NativeLvAnalysisValidationRequestV1,
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<LvAnalysisValidationOutcome, NetworkFailure> {
        let input = serde_json::to_vec(request)
            .map_err(|error| NetworkFailure::new("network_input_invalid", error.to_string()))?;
        decode_native_lv_analysis_validation_request(&input).map_err(NetworkFailure::native)?;
        let (bytes, receipt) =
            self.saved_lv_task(SavedTask::Validate { request: &input }, scratch, cancel)?;
        let result: NativeLvAnalysisValidationResultV1 = serde_json::from_slice(&bytes)
            .map_err(|error| NetworkFailure::new("network_result_invalid", error.to_string()))?;
        if result.schema != NATIVE_LV_ANALYSIS_VALIDATION_RESULT_SCHEMA_V1
            || result.transformer_names
                != request
                    .jobs
                    .iter()
                    .map(|job| job.source.transformer_name.clone())
                    .collect::<Vec<_>>()
        {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "validation facts disagree with the ordered save inputs",
            ));
        }
        Ok(LvAnalysisValidationOutcome {
            bytes,
            receipt,
            result,
        })
    }

    fn saved_lv_task(
        &self,
        task: SavedTask<'_>,
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<(Vec<u8>, NetworkReceiptV1), NetworkFailure> {
        let (command, capability, input, artifact, schema, retained, input_maximum) = match task {
            SavedTask::Project {
                request,
                result,
                transformer,
                schema,
            } => (
                "lv-project",
                NetworkCapability::LvPublicationV1,
                request,
                "publication.json",
                schema,
                Some((result, transformer)),
                MAX_NATIVE_LV_INPUT_BYTES,
            ),
            SavedTask::Validate { request } => (
                "lv-analysis-validate",
                NetworkCapability::LvAnalysisValidationV1,
                request,
                "validation.json",
                NATIVE_LV_ANALYSIS_VALIDATION_RESULT_SCHEMA_V1,
                None,
                MAX_NATIVE_LV_ANALYSIS_VALIDATION_BYTES,
            ),
        };
        if !self.identity.capabilities.contains(&capability) {
            return Err(NetworkFailure::new(
                "network_engine_incompatible",
                format!("captured Network engine does not admit {command}"),
            ));
        }
        let sm = fs::symlink_metadata(scratch)
            .map_err(|error| NetworkFailure::new("network_scratch_invalid", error.to_string()))?;
        if !sm.is_dir() || sm.file_type().is_symlink() {
            return Err(NetworkFailure::new(
                "network_scratch_invalid",
                "scratch must be an ordinary directory",
            ));
        }
        let run = tempfile::Builder::new()
            .prefix("ds-network-lv-save-")
            .tempdir_in(scratch)
            .map_err(|error| NetworkFailure::new("network_scratch_invalid", error.to_string()))?;
        let input_path = run.path().join("request.json");
        write_request(&input_path, input)?;
        let output = run.path().join("output");
        create_output(&output)?;
        let width = self.identity.runtime.internal_worker_threads;
        let mut args: Vec<OsString> = vec![
            "--request".into(),
            input_path.as_os_str().to_owned(),
            "--out".into(),
            output.as_os_str().to_owned(),
            "--workers".into(),
            width.to_string().into(),
        ];
        let result_path = run.path().join("result.json");
        let result_sha = if let Some((result, transformer)) = retained {
            write_request(&result_path, result)?;
            args.extend([
                "--result".into(),
                result_path.as_os_str().to_owned(),
                "--transformer".into(),
                transformer.into(),
            ]);
            Some(digest_bytes(result))
        } else {
            None
        };
        let completed = self
            .engine
            .call_cancelled(command, &args, TASK_TIMEOUT, width, cancel)
            .map_err(|error| NetworkFailure::new(static_code(error.code()), error.to_string()))?;
        if completed.truncated || completed.stdout.len() > MAX_IDENTITY_BYTES {
            return Err(NetworkFailure::new(
                "network_receipt_bounded",
                "LV save task exceeded its terminal receipt bound",
            ));
        }
        if !completed.succeeded() {
            return Err(read_refusal(&completed.stdout, &self.identity)?);
        }
        let receipt: NetworkReceiptV1 = serde_json::from_str(&completed.stdout)
            .map_err(|error| NetworkFailure::new("network_receipt_invalid", error.to_string()))?;
        let input_sha = digest_bytes(input);
        if receipt.schema != "ds.network.task-result/v1"
            || receipt.input_sha256 != input_sha
            || !same_identity(&receipt.engine_identity, &self.identity)?
            || receipt.result_input_sha256 != result_sha
            || receipt.artifact.file != artifact
            || receipt.artifact.schema != schema
            || receipt.artifact.byte_count == 0
            || receipt.artifact.byte_count > MAX_NATIVE_LV_OUTPUT_BYTES
            || !valid_hex(&receipt.artifact.sha256, 64)
        {
            return Err(NetworkFailure::new("network_receipt_invalid", "LV save receipt differs from the exact input, retained result, engine or fixed artifact"));
        }
        let om = fs::symlink_metadata(&output)
            .map_err(|error| NetworkFailure::new("network_result_invalid", error.to_string()))?;
        if !om.is_dir() || om.file_type().is_symlink() {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "output directory changed",
            ));
        }
        let outputs = fs::read_dir(&output)
            .map_err(|error| NetworkFailure::new("network_result_invalid", error.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| NetworkFailure::new("network_result_invalid", error.to_string()))?;
        if outputs.len() != 1 || outputs[0].file_name() != artifact {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "LV save task produced undeclared artifacts",
            ));
        }
        let bytes = read_regular(
            &output.join(artifact),
            MAX_NATIVE_LV_OUTPUT_BYTES,
            "network_result_invalid",
        )?;
        if bytes.len() != receipt.artifact.byte_count
            || digest_bytes(&bytes) != receipt.artifact.sha256
        {
            return Err(NetworkFailure::new(
                "network_result_digest_mismatch",
                "LV save artifact differs from its receipt",
            ));
        }
        if digest_file(&input_path, input_maximum as u64, "network_input_changed")? != input_sha
            || result_sha.as_ref().is_some_and(|expected| {
                digest_file(
                    &result_path,
                    MAX_NATIVE_LV_OUTPUT_BYTES as u64,
                    "network_input_changed",
                )
                .map_or(true, |sha| sha != *expected)
            })
        {
            return Err(NetworkFailure::new(
                "network_input_changed",
                "staged LV save inputs changed during execution",
            ));
        }
        if digest_file(
            self.engine.executable(),
            MAX_EXECUTABLE_BYTES,
            "network_engine_changed",
        )? != self.executable_sha256
        {
            return Err(NetworkFailure::new(
                "network_engine_changed",
                "captured Network executable changed during execution",
            ));
        }
        if cancel() || matches!(receipt.outcome, NetworkTaskOutcome::Cancelled) {
            return Err(NetworkFailure::new(
                "callee_cancelled",
                "LV save task was cancelled",
            ));
        }
        Ok((bytes, receipt))
    }
}
