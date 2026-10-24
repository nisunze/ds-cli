//! Local GIS source calls: the host stages one admitted file's exact bytes,
//! the captured engine reads it through the named reader, and only its
//! complete, digest-bound reply is admitted here. Hosts keep the file,
//! its admission and every placement of what comes back.
use super::*;
use ds_network_contracts::{
    grid_operation::GridOperationV1,
    source_io::{
        GeopackageLayersParamsV1, GeopackageLayersV1, SourceInspectParamsV1, SourceInspectResultV1,
        SourceInspectV1, SourceLayersParamsV1, SourceLayersV1, SourceReaderV1,
        GEOPACKAGE_LAYERS_ARTIFACT, GEOPACKAGE_LAYERS_SCHEMA_V1, SOURCE_INPUT,
        SOURCE_LAYERS_ARTIFACT, SOURCE_LAYERS_SCHEMA_V1,
    },
};

/// The owner code a reader's own refusal carries; its message is the
/// reader's words, unchanged.
pub const SOURCE_UNREADABLE: &str = "source_unreadable";

/// Source-read failure descriptions owned by the native Network client.
/// No CLI class, constructor or command dependency lives in this vocabulary.
#[derive(Clone, Copy, Debug)]
pub struct SourceEngineRefusal {
    pub code: &'static str,
    pub when: &'static str,
    pub remedy: &'static str,
}

/// Legacy host codes and shared owner causes accepted by source-read calls.
/// LV/snapshot causes are retained for compatibility with shared refusal parsing;
/// they do not claim that a GIS read performs those operations.
pub const SOURCE_ENGINE_REFUSALS: [SourceEngineRefusal; 40] = [
    SourceEngineRefusal {
        code: "network_engine_missing",
        when: "the Network addon executable is absent",
        remedy: "install Network or set DS_NETWORK_ENGINE_BIN to a compatible executable",
    },
    SourceEngineRefusal {
        code: "network_engine_incompatible",
        when: "the captured Network protocol or capability is incompatible",
        remedy: "select a compatible Network addon",
    },
    SourceEngineRefusal {
        code: "network_engine_path_invalid",
        when: "the selected executable path is invalid",
        remedy: "use an absolute ordinary executable",
    },
    SourceEngineRefusal {
        code: "network_engine_invalid",
        when: "the executable cannot be admitted",
        remedy: "install an intact compatible addon",
    },
    SourceEngineRefusal {
        code: "network_engine_unqualified",
        when: "the selected component has no qualified identity",
        remedy: "install a verified release; development overrides remain explicit",
    },
    SourceEngineRefusal {
        code: "network_engine_identity_invalid",
        when: "build-info is malformed or unbounded",
        remedy: "install a matching Network release",
    },
    SourceEngineRefusal {
        code: "network_engine_changed",
        when: "the captured executable changed during execution",
        remedy: "retain immutable version directories and retry unchanged input",
    },
    SourceEngineRefusal {
        code: "network_engine_refused",
        when: "the engine refuses its fixed task contract",
        remedy: "read the owning refusal and use a compatible component",
    },
    SourceEngineRefusal {
        code: "network_workers_invalid",
        when: "the worker budget cannot be admitted",
        remedy: "use a positive bounded worker allocation",
    },
    SourceEngineRefusal {
        code: "network_serialization_incompatible",
        when: "the GIS engine sorts JSON members",
        remedy: "select the insertion-order Network component",
    },
    SourceEngineRefusal {
        code: "network_scratch_invalid",
        when: "private disk staging cannot be admitted",
        remedy: "provide a writable ordinary disk-backed cache directory",
    },
    SourceEngineRefusal {
        code: "network_request_invalid",
        when: "the fixed source-read request cannot be encoded",
        remedy: "retain the source and use a compatible Network component",
    },
    SourceEngineRefusal {
        code: "network_request_io",
        when: "the captured request cannot be staged",
        remedy: "provide writable disk space",
    },
    SourceEngineRefusal {
        code: "grid_task_bound_refused",
        when: "the opaque model or complete native artifact exceeds its bound",
        remedy: "use one bounded model or operation",
    },
    SourceEngineRefusal {
        code: "network_receipt_bounded",
        when: "the terminal receipt exceeds its bound",
        remedy: "use a compatible bounded Network task",
    },
    SourceEngineRefusal {
        code: "network_receipt_invalid",
        when: "the receipt differs from captured inputs or identity",
        remedy: "install a matching intact component",
    },
    SourceEngineRefusal {
        code: "network_refusal_invalid",
        when: "the engine refusal has an incompatible identity or shape",
        remedy: "use a compatible component",
    },
    SourceEngineRefusal {
        code: "network_result_invalid",
        when: "the complete artifact or progress does not match its contract",
        remedy: "use an intact compatible addon and unchanged inputs",
    },
    SourceEngineRefusal {
        code: "network_result_digest_mismatch",
        when: "the returned artifact digest or size differs from the receipt",
        remedy: "retry unchanged inputs using an intact component",
    },
    SourceEngineRefusal {
        code: "network_input_changed",
        when: "a staged input changed during execution",
        remedy: "retry from retained unchanged inputs",
    },
    SourceEngineRefusal {
        code: "callee_cancelled",
        when: "the host cancels the supervised child",
        remedy: "retry unchanged input explicitly",
    },
    SourceEngineRefusal {
        code: "callee_timed_out",
        when: "the identity probe or task exceeds its deadline",
        remedy: "split large work or select a compatible addon",
    },
    SourceEngineRefusal {
        code: "callee_wait_failed",
        when: "the host cannot reap its owned process",
        remedy: "inspect host process availability",
    },
    SourceEngineRefusal {
        code: "callee_supervision_failed",
        when: "process-tree supervision cannot be established",
        remedy: "use a host supporting the admitted native boundary",
    },
    SourceEngineRefusal {
        code: "network_task_cancelled",
        when: "the owning task was cancelled",
        remedy: "retry the unchanged input explicitly",
    },
    SourceEngineRefusal {
        code: "network_control_unavailable",
        when: "the task cancellation channel cannot be opened",
        remedy: "use a compatible native supervision host",
    },
    SourceEngineRefusal {
        code: "network_control_invalid",
        when: "the typed cancellation channel is invalid",
        remedy: "use matching host and component contracts",
    },
    SourceEngineRefusal {
        code: "network_input_invalid",
        when: "a fixed native task input cannot be admitted",
        remedy: "use matching prepared inputs",
    },
    SourceEngineRefusal {
        code: "network_output_invalid",
        when: "the fixed output directory cannot be admitted",
        remedy: "provide writable private disk space",
    },
    SourceEngineRefusal {
        code: "network_artifact_io",
        when: "the engine cannot write its complete artifact",
        remedy: "provide writable private disk space",
    },
    SourceEngineRefusal {
        code: "callee_stdin_failed",
        when: "the child cannot receive its fixed control input",
        remedy: "use an intact compatible component",
    },
    SourceEngineRefusal {
        code: "fast_lv_input_too_large",
        when: "the engine reports a retained legacy owner failure",
        remedy: "retain the source and use a compatible Network component",
    },
    SourceEngineRefusal {
        code: "fast_lv_input_invalid",
        when: "the engine reports a retained legacy owner failure",
        remedy: "retain the source and use a compatible Network component",
    },
    SourceEngineRefusal {
        code: "fast_lv_schema_unsupported",
        when: "the engine reports a retained legacy owner failure",
        remedy: "retain the source and use a compatible Network component",
    },
    SourceEngineRefusal {
        code: "fast_lv_bound_refused",
        when: "the engine reports a retained legacy owner failure",
        remedy: "retain the source and use a compatible Network component",
    },
    SourceEngineRefusal {
        code: "fast_lv_output_too_large",
        when: "the engine reports a retained legacy owner failure",
        remedy: "retain the source and use a compatible Network component",
    },
    SourceEngineRefusal {
        code: "fast_lv_result_encoding_failed",
        when: "the engine reports a retained legacy owner failure",
        remedy: "retain the source and use a compatible Network component",
    },
    SourceEngineRefusal {
        code: "fast_lv_publication_invalid",
        when: "the engine reports a retained legacy owner failure",
        remedy: "retain the source and use a compatible Network component",
    },
    SourceEngineRefusal {
        code: "design_snapshot_processing_failed",
        when: "the engine reports a retained legacy owner failure",
        remedy: "retain the source and use a compatible Network component",
    },
    SourceEngineRefusal {
        code: "network_engine_refusal_unknown",
        when: "the engine returns an unrecognized source-read cause",
        remedy: "retain the bounded cause and use a compatible Network component",
    },
];

fn bounded_source_text(text: &str, limit: usize) -> String {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn reply_invalid(message: &str) -> NetworkFailure {
    NetworkFailure::new("network_result_invalid", message)
}

impl NetworkFailure {
    /// The reader refused the source itself (its message is the reader's).
    pub fn is_source_refusal(&self) -> bool {
        self.owner_code() == Some(SOURCE_UNREADABLE)
    }

    /// Original source-read cause, bounded at UTF-8 boundaries for an unknown refusal.
    pub fn bounded_source_cause(&self) -> serde_json::Value {
        let code = self.owner_code().unwrap_or(self.code());
        serde_json::json!({
            "engine_code": bounded_source_text(code, 128),
            "cause": bounded_source_text(&self.message, 4096),
            "code_truncated": code.len() > 128,
            "cause_truncated": self.message.len() > 4096,
        })
    }
}

impl CapturedNetworkClient {
    fn source_call(
        &self,
        call: GridOperationV1,
        input: &str,
        bytes: Vec<u8>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GridOperationOutcome, NetworkFailure> {
        let stage = private_disk_stage()?;
        self.grid_operation(
            &call,
            None,
            &[(input.to_owned(), bytes)],
            stage.path(),
            cancel,
        )
    }

    /// A table's sheets, or the layer summaries of a source that carries
    /// its own geometry.
    pub fn source_inspect(
        &self,
        name: &str,
        bytes: Vec<u8>,
        inspect: SourceInspectV1,
        cancel: &dyn Fn() -> bool,
    ) -> Result<SourceInspectResultV1, NetworkFailure> {
        let call = GridOperationV1::SourceInspect(SourceInspectParamsV1 {
            name: name.to_owned(),
            inspect,
        });
        let outcome = self.source_call(call, SOURCE_INPUT, bytes, cancel)?;
        serde_json::from_value(outcome.result.result)
            .map_err(|_| reply_invalid("source inspection reply is not the typed result"))
    }

    /// The canonical layers one named reader produces.
    pub fn source_layers(
        &self,
        name: &str,
        bytes: Vec<u8>,
        reader: SourceReaderV1,
        cancel: &dyn Fn() -> bool,
    ) -> Result<SourceLayersV1, NetworkFailure> {
        let call = GridOperationV1::SourceLayers(SourceLayersParamsV1 {
            name: name.to_owned(),
            reader,
        });
        let mut outcome = self.source_call(call, SOURCE_INPUT, bytes, cancel)?;
        let artifact = outcome
            .take_artifact(SOURCE_LAYERS_ARTIFACT)
            .ok_or_else(|| reply_invalid("source layers reply has no layers artifact"))?;
        let layers: SourceLayersV1 = serde_json::from_slice(&artifact)
            .map_err(|_| reply_invalid("source layers artifact is not the typed document"))?;
        if !same_family(&layers.schema, SOURCE_LAYERS_SCHEMA_V1) {
            return Err(reply_invalid("source layers artifact names another schema"));
        }
        Ok(layers)
    }

    /// Named feature tables of one closed GeoPackage the host admitted.
    pub fn geopackage_layers(
        &self,
        bytes: Vec<u8>,
        requested: &[String],
        max_rows: usize,
        max_bytes: usize,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GeopackageLayersV1, NetworkFailure> {
        let call = GridOperationV1::GeopackageLayers(GeopackageLayersParamsV1 {
            requested: requested.to_vec(),
            max_rows,
            max_bytes,
        });
        let source_sha256 = digest_bytes(&bytes);
        let mut outcome = self.source_call(call, SOURCE_INPUT, bytes, cancel)?;
        let artifact = outcome
            .take_artifact(GEOPACKAGE_LAYERS_ARTIFACT)
            .ok_or_else(|| reply_invalid("GeoPackage reply has no tables artifact"))?;
        let tables: GeopackageLayersV1 = serde_json::from_slice(&artifact)
            .map_err(|_| reply_invalid("GeoPackage artifact is not the typed document"))?;
        if !same_family(&tables.schema, GEOPACKAGE_LAYERS_SCHEMA_V1)
            || tables.source_sha256 != source_sha256
            || tables.layers.keys().ne(tables.feature_counts.keys())
            || requested
                .iter()
                .any(|name| !tables.layers.contains_key(name))
        {
            return Err(reply_invalid(
                "GeoPackage tables address another source or other tables",
            ));
        }
        Ok(tables)
    }
}

#[cfg(test)]
#[path = "source_io_tests.rs"]
mod tests;
