//! The Reporter's print layout editor, as Core's layout commands call it.
//!
//! A layout edit — pens, paper, furniture, frames, placement areas, tables,
//! context layers, adjacent circuits, style resolution — is printing
//! computation, so the Reporter's `edit_print_layout` task decides it. Core
//! hands over the caller's own edit document unchanged, inside the task's
//! envelope, and returns the editor's answer or its refusal message exactly as
//! the editor wrote it. A Reporter without the task refuses `unknown_task`
//! before any layout file is read; operations the Kernel evaluator owns
//! (sessions, output selection, inventory, lifecycle, validation, new
//! layouts) never reach the Reporter.
use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::Refusal;
use serde_json::Value;
use std::time::Duration;

const REQUEST_SCHEMA: &str = "ds.print-layout-edit.request/v1";
const RESULT_SCHEMA: &str = "ds.print-layout-edit/v1";
/// A layout edit is a bounded in-memory computation over one document.
const EDIT_TIMEOUT: Duration = Duration::from_secs(60);

/// The Reporter refusals every editor command can return besides its own.
pub(crate) const ENGINE_REFUSALS: [Refusal; 5] = [
    crate::ENGINE_MISSING,
    crate::ENGINE_RECEIPT_INVALID,
    crate::ENGINE_TASK_UNKNOWN,
    Refusal {
        code: "engine_refused",
        when: "the Reporter failed the layout edit task itself",
        remedy: "read detail.engine; reinstall the Reporter for the selected --lane if it persists",
    },
    Refusal {
        code: "callee_contract_mismatch",
        when: "the Reporter answered, but not with a layout edit answer",
        remedy: "install a Reporter for the selected --lane that registers edit_print_layout",
    },
];

/// One command's editor: the selected lane's Reporter, captured once and
/// admitted for `edit_print_layout` before any input is read.
pub(crate) struct Editor {
    engine: crate::Reporter,
}

impl Editor {
    pub(crate) fn capture(inputs: &Inputs) -> Result<Self, Failure> {
        let engine = crate::capture_engine(inputs)?;
        engine.require(crate::ReporterTask::EditPrintLayout)?;
        Ok(Self { engine })
    }

    /// Apply one edit document (`{op, …}`): the editor's answer, or
    /// `Err(message)` for the edit it refused.
    pub(crate) fn edit(&self, edit: &[u8]) -> Result<Result<Value, String>, Failure> {
        let scratch = tempfile::tempdir().map_err(unavailable)?;
        let (request, result) = (
            scratch.path().join("layout-edit-request.json"),
            scratch.path().join("layout-edit-result.json"),
        );
        // The caller's bytes, unchanged: a refusal names their own positions.
        let mut body = format!(r#"{{"schema":"{REQUEST_SCHEMA}","edit":"#).into_bytes();
        body.extend_from_slice(edit);
        body.push(b'}');
        std::fs::write(&request, body).map_err(unavailable)?;
        let reply = self.engine.run(
            crate::ReporterTask::EditPrintLayout,
            &request,
            &result,
            EDIT_TIMEOUT,
        )?;
        if !reply.succeeded() {
            return Err(reply.refusal());
        }
        let mut answer: Value = std::fs::read(&result)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or_else(|| mismatch("the Reporter wrote no readable layout edit answer"))?;
        if answer["schema"] != RESULT_SCHEMA {
            return Err(mismatch(
                "the Reporter answered another layout edit document",
            ));
        }
        if let Some(refusal) = answer.get("refusal") {
            return Ok(Err(refusal
                .as_str()
                .ok_or_else(|| mismatch("the Reporter's layout edit refusal is not text"))?
                .to_owned()));
        }
        match answer.get_mut("result").map(Value::take) {
            Some(result) => Ok(Ok(result)),
            None => Err(mismatch(
                "the Reporter's layout edit answer carries no result",
            )),
        }
    }

    /// Apply one edit document built by the command.
    pub(crate) fn edit_value(&self, edit: &Value) -> Result<Result<Value, String>, Failure> {
        self.edit(&serde_json::to_vec(edit).map_err(unavailable)?)
    }
}

fn mismatch(message: &str) -> Failure {
    Failure::failed("callee_contract_mismatch", message)
        .remedy("install a Reporter for the selected --lane that registers edit_print_layout")
}

fn unavailable(error: impl std::fmt::Display) -> Failure {
    Failure::failed(
        "engine_refused",
        format!("the layout edit could not be staged: {error}"),
    )
    .remedy("read detail.engine; reinstall the Reporter for the selected --lane if it persists")
}
