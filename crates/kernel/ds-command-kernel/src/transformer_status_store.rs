//! The shared whole-project status observation owner. Hosts execute bounded SQL;
//! no storage fallback, timer, credential, or network IO lives here.
use crate::sql_effect::{ResultRows, Statement};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const MAX_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS project_status_observations (scope TEXT PRIMARY KEY NOT NULL, document TEXT, sha256 TEXT, observed_at_ms INTEGER, needs_refresh INTEGER NOT NULL CHECK(needs_refresh IN (0,1)), last_error TEXT, revision INTEGER NOT NULL CHECK(revision>0 AND revision<=9007199254740991), CHECK((document IS NULL AND sha256 IS NULL AND observed_at_ms IS NULL) OR (document IS NOT NULL AND sha256 IS NOT NULL AND observed_at_ms IS NOT NULL)))";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub principal: String,
    pub lane: String,
    pub audience: String,
    pub project: String,
}
impl Scope {
    pub fn key(&self) -> Result<String, String> {
        if [&self.principal, &self.lane, &self.audience, &self.project]
            .iter()
            .any(|s| s.is_empty() || s.len() > 500 || s.trim() != *s)
        {
            return Err("transformer status needs a complete identity and project fence".into());
        }
        serde_json::to_string(self).map_err(|e| e.to_string())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Observation {
    pub document: Value,
    pub sha256: String,
    pub observed_at_ms: u64,
    pub needs_refresh: bool,
    pub last_error: Option<String>,
    pub revision: u64,
}
pub fn digest(value: &Value) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|e| e.to_string())?)
    ))
}
pub fn schema() -> Statement {
    Statement::new(SCHEMA, vec![], 0, 2)
}
pub fn size(scope: &Scope) -> Result<Statement, String> {
    Ok(Statement::new(
        "SELECT revision,length(CAST(document AS BLOB)) FROM project_status_observations WHERE scope=?1",
        vec![json!(scope.key()?)],
        1,
        128,
    ))
}
pub fn read(scope: &Scope) -> Result<Statement, String> {
    Ok(Statement::new("SELECT document,sha256,observed_at_ms,needs_refresh,last_error,revision FROM project_status_observations WHERE scope=?1 AND length(CAST(document AS BLOB))<=?2",vec![json!(scope.key()?),json!(MAX_BYTES)],1,64*1024*1024))
}
pub fn admit_size(result: &ResultRows) -> Result<(Option<u64>, bool), String> {
    match result.rows.as_slice() {
        [] => Ok((None, false)),
        [row] if row.len() == 2 => {
            let revision = row[0]
                .as_u64()
                .filter(|n| *n > 0 && *n <= 9_007_199_254_740_991)
                .ok_or("invalid status revision")?;
            if row[1].is_null() {
                return Ok((Some(revision), false));
            }
            if row[1].as_u64().is_none_or(|v| v > MAX_BYTES as u64) {
                return Err("transformer status observation exceeds its byte bound".into());
            }
            Ok((Some(revision), true))
        }
        _ => Err("invalid status size row".into()),
    }
}
pub fn decode(result: &ResultRows) -> Result<Option<Observation>, String> {
    let Some(row) = result.rows.first() else {
        return Ok(None);
    };
    if result.rows.len() != 1 || row.len() != 6 {
        return Err("invalid status observation row".into());
    }
    let text = row[0].as_str().ok_or("invalid status document")?;
    if text.len() > MAX_BYTES {
        return Err("status document exceeds byte bound".into());
    }
    let document: Value = serde_json::from_str(text).map_err(|_| "status document is corrupt")?;
    let sha256 = row[1].as_str().ok_or("invalid status digest")?.to_string();
    if digest(&document)? != sha256 {
        return Err("transformer status observation digest mismatch".into());
    }
    let observed_at_ms = row[2]
        .as_u64()
        .filter(|n| *n <= 9_007_199_254_740_991)
        .ok_or("invalid status observation time")?;
    let needs_refresh = match row[3].as_u64() {
        Some(0) => false,
        Some(1) => true,
        _ => return Err("invalid status read state".into()),
    };
    let last_error = if row[4].is_null() {
        None
    } else {
        Some(
            row[4]
                .as_str()
                .filter(|s| *s == "refresh_failed")
                .ok_or("invalid status error")?
                .into(),
        )
    };
    Ok(Some(Observation {
        document,
        sha256,
        observed_at_ms,
        needs_refresh,
        last_error,
        revision: row[5]
            .as_u64()
            .filter(|n| *n > 0 && *n <= 9_007_199_254_740_991)
            .ok_or("invalid status revision")?,
    }))
}
pub fn write(scope: &Scope, document: &Value, now: u64) -> Result<Statement, String> {
    let bytes = serde_json::to_string(document).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES || now > 9_007_199_254_740_991 {
        return Err("status observation exceeds its bounds".into());
    }
    Ok(Statement::new("INSERT INTO project_status_observations VALUES(?1,?2,?3,?4,0,NULL,1) ON CONFLICT(scope) DO UPDATE SET document=excluded.document,sha256=excluded.sha256,observed_at_ms=excluded.observed_at_ms,needs_refresh=0,last_error=NULL,revision=project_status_observations.revision+1",vec![json!(scope.key()?),json!(bytes),json!(digest(document)?),json!(now)],0,2))
}
fn revision(scope: &Scope) -> Result<Statement, String> {
    Ok(Statement::new(
        "SELECT revision FROM project_status_observations WHERE scope=?1",
        vec![json!(scope.key()?)],
        1,
        128,
    ))
}
pub fn invalidate(scope: &Scope, failed: bool) -> Result<Statement, String> {
    Ok(Statement::new(
        "INSERT INTO project_status_observations VALUES(?1,NULL,NULL,NULL,1,?2,1) ON CONFLICT(scope) DO UPDATE SET needs_refresh=1,last_error=excluded.last_error,revision=project_status_observations.revision+1",
        vec![
            json!(scope.key()?),
            if failed {
                json!("refresh_failed")
            } else {
                Value::Null
            },
        ],
        0,
        2,
    ))
}
/// Closed status-only continuation. Each operation runs in one host transaction.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Read {
        scope: Scope,
    },
    Write {
        scope: Scope,
        document: Value,
        now: u64,
    },
    Invalidate {
        scope: Scope,
        failed: bool,
    },
    Commit {
        scope: Scope,
        document: Value,
        now: u64,
        expected: Option<u64>,
    },
    MarkFailed {
        scope: Scope,
        expected: Option<u64>,
    },
    Schema,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub enum Continuation {
    Size {
        scope: Scope,
    },
    Read,
    Commit {
        scope: Scope,
        document: Option<Value>,
        now: u64,
        expected: Option<u64>,
    },
    Finish,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Step {
    Sql {
        statement: Statement,
        continuation: Continuation,
    },
    Done {
        observation: Option<Observation>,
        revision: Option<u64>,
    },
}
pub fn start(operation: Operation) -> Result<Step, String> {
    Ok(match operation {
        Operation::Read { scope } => Step::Sql {
            statement: size(&scope)?,
            continuation: Continuation::Size { scope },
        },
        Operation::Write {
            scope,
            document,
            now,
        } => Step::Sql {
            statement: write(&scope, &document, now)?,
            continuation: Continuation::Finish,
        },
        Operation::Invalidate { scope, failed } => Step::Sql {
            statement: invalidate(&scope, failed)?,
            continuation: Continuation::Finish,
        },
        Operation::Commit {
            scope,
            document,
            now,
            expected,
        } => Step::Sql {
            statement: revision(&scope)?,
            continuation: Continuation::Commit {
                scope,
                document: Some(document),
                now,
                expected,
            },
        },
        Operation::MarkFailed { scope, expected } => Step::Sql {
            statement: revision(&scope)?,
            continuation: Continuation::Commit {
                scope,
                document: None,
                now: 0,
                expected,
            },
        },
        Operation::Schema => Step::Sql {
            statement: schema(),
            continuation: Continuation::Finish,
        },
    })
}
pub fn resume(continuation: Continuation, result: ResultRows) -> Result<Step, String> {
    Ok(match continuation {
        Continuation::Size { scope } => {
            let (revision, present) = admit_size(&result)?;
            if present {
                Step::Sql {
                    statement: read(&scope)?,
                    continuation: Continuation::Read,
                }
            } else {
                Step::Done {
                    observation: None,
                    revision,
                }
            }
        }
        Continuation::Read => {
            let observation = decode(&result)?;
            Step::Done {
                revision: observation.as_ref().map(|v| v.revision),
                observation,
            }
        }
        Continuation::Commit {
            scope,
            document,
            now,
            expected,
        } => {
            let current = match result.rows.as_slice() {
                [] => None,
                [row] if row.len() == 1 => Some(row[0].as_u64().ok_or("invalid status revision")?),
                _ => return Err("invalid status revision rows".into()),
            };
            if current != expected {
                if document.is_some() {
                    return Err("status_refresh_superseded".into());
                }
                Step::Done {
                    observation: None,
                    revision: None,
                }
            } else {
                Step::Sql {
                    statement: if let Some(document) = document {
                        write(&scope, &document, now)?
                    } else {
                        invalidate(&scope, true)?
                    },
                    continuation: Continuation::Finish,
                }
            }
        }
        Continuation::Finish => {
            if !result.rows.is_empty() {
                return Err("unexpected status write rows".into());
            }
            Step::Done {
                observation: None,
                revision: None,
            }
        }
    })
}
/// Fixed gateway request shared with the native authenticated client.
pub const READ_TIMEOUT_SECONDS: u64 = 120;
pub fn request_body(project: &str, targets: &[String]) -> Value {
    let mut body = json!({"action":"list_transformers_status","eds_project_id":project});
    if !targets.is_empty() {
        body["transformers"] = json!(targets);
    }
    body.sort_all_objects();
    body
}
pub fn admit_rows(value: Value) -> Result<Vec<Value>, String> {
    let rows = match value {
        Value::Null => vec![],
        Value::Array(rows) => rows,
        _ => return Err("invalid transformer status rows".into()),
    };
    if rows.len() > 2000 {
        return Err("transformer status row count exceeds its bound".into());
    }
    let bounded = |value: &str, max: usize| {
        !value.is_empty()
            && value.trim() == value
            && value.chars().count() <= max
            && !value
                .chars()
                .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
    };
    for row in &rows {
        let object = row.as_object().ok_or("invalid transformer status row")?;
        if !object
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| bounded(name, crate::report::transformers::MAX_NAME_CHARS))
        {
            return Err("invalid transformer name".into());
        }
        match object.get("kind") {
            None | Some(Value::Null) => (),
            Some(Value::String(kind)) if bounded(kind, 64) => (),
            _ => return Err("invalid transformer kind".into()),
        }
    }
    Ok(rows)
}
pub fn admit_response(mut document: Value) -> Result<Value, String> {
    if document.get("error").is_some_and(|v| !v.is_null()) {
        return Err("status_refresh_failed".into());
    }
    if serde_json::to_vec(&document)
        .map_err(|e| e.to_string())?
        .len()
        > MAX_RESPONSE_BYTES
    {
        return Err("status response exceeds byte bound".into());
    }
    let rows = document
        .as_object_mut()
        .and_then(|object| object.remove("transformers"))
        .ok_or("status response has no transformers")?;
    document["transformers"] = json!(admit_rows(rows)?);
    Ok(document)
}
pub fn needs_acquisition(refresh: bool, held: Option<&Observation>) -> bool {
    refresh || held.is_none_or(|v| v.needs_refresh)
}
pub fn view(
    held: Option<Observation>,
    refreshed: bool,
    targets: &[String],
) -> Result<Value, String> {
    let held = held.ok_or("design_status_not_held")?;
    let rows = admit_rows(
        held.document
            .get("transformers")
            .cloned()
            .ok_or("status observation has no transformers")?,
    )?;
    let rows: Vec<_> = rows
        .into_iter()
        .filter(|row| {
            targets.is_empty()
                || row
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| targets.iter().any(|target| target == name))
        })
        .collect();
    let state = if held.needs_refresh || held.last_error.is_some() {
        "held"
    } else {
        "ready"
    };
    Ok(
        json!({"data":rows,"observation":{"source":if refreshed{"refresh"}else{"held"},"state":state,"observed_at_ms":held.observed_at_ms,"sha256":held.sha256,"last_error":held.last_error},"error":if state=="held"{Some("Status is held; refresh to observe current project status.")}else{None}}),
    )
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadMode {
    Held,
    Refresh,
    Acquire,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum FlowContinuation {
    Held {
        scope: Scope,
        mode: ReadMode,
    },
    Fetched {
        scope: Scope,
        expected: Option<u64>,
    },
    Stored {
        scope: Scope,
        refreshed: bool,
        failed: bool,
    },
    Finish {
        refreshed: bool,
        failed: bool,
    },
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FlowStep {
    Storage {
        operation: Operation,
        continuation: FlowContinuation,
    },
    Fetch {
        route: String,
        body: Value,
        timeout_ms: u64,
        continuation: FlowContinuation,
    },
    Done {
        observation: Option<Observation>,
        refreshed: bool,
        failed: bool,
    },
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FlowResult {
    Storage {
        observation: Option<Observation>,
        revision: Option<u64>,
    },
    Fetch {
        document: Value,
        now: u64,
    },
    Failure,
}
pub fn flow_start(scope: Scope, mode: ReadMode) -> Result<FlowStep, String> {
    scope.key()?;
    Ok(FlowStep::Storage {
        operation: Operation::Read {
            scope: scope.clone(),
        },
        continuation: FlowContinuation::Held { scope, mode },
    })
}
pub fn flow_resume(continuation: FlowContinuation, result: FlowResult) -> Result<FlowStep, String> {
    Ok(match (continuation, result) {
        (
            FlowContinuation::Held { scope, mode },
            FlowResult::Storage {
                observation,
                revision,
            },
        ) => {
            if matches!(mode, ReadMode::Refresh)
                || matches!(mode, ReadMode::Acquire)
                    && needs_acquisition(false, observation.as_ref())
            {
                FlowStep::Fetch {
                    route: "/report".into(),
                    body: request_body(&scope.project, &[]),
                    timeout_ms: READ_TIMEOUT_SECONDS * 1000,
                    continuation: FlowContinuation::Fetched {
                        scope,
                        expected: revision,
                    },
                }
            } else {
                FlowStep::Done {
                    observation,
                    refreshed: false,
                    failed: false,
                }
            }
        }
        (FlowContinuation::Fetched { scope, expected }, FlowResult::Fetch { document, now }) => {
            let document = match admit_response(document) {
                Ok(document) => document,
                Err(_) => {
                    return flow_resume(
                        FlowContinuation::Fetched { scope, expected },
                        FlowResult::Failure,
                    )
                }
            };
            FlowStep::Storage {
                operation: Operation::Commit {
                    scope: scope.clone(),
                    document,
                    now,
                    expected,
                },
                continuation: FlowContinuation::Stored {
                    scope,
                    refreshed: true,
                    failed: false,
                },
            }
        }
        (FlowContinuation::Fetched { scope, expected }, FlowResult::Failure) => FlowStep::Storage {
            operation: Operation::MarkFailed {
                scope: scope.clone(),
                expected,
            },
            continuation: FlowContinuation::Stored {
                scope,
                refreshed: false,
                failed: true,
            },
        },
        (
            FlowContinuation::Stored {
                scope,
                refreshed,
                failed,
            },
            FlowResult::Storage { .. },
        ) => FlowStep::Storage {
            operation: Operation::Read { scope },
            continuation: FlowContinuation::Finish { refreshed, failed },
        },
        (
            FlowContinuation::Finish { refreshed, failed },
            FlowResult::Storage { observation, .. },
        ) => FlowStep::Done {
            observation,
            refreshed,
            failed,
        },
        _ => return Err("invalid status acquisition continuation".into()),
    })
}
#[derive(Deserialize)]
#[serde(tag = "call", rename_all = "snake_case", deny_unknown_fields)]
enum Call {
    Start {
        operation: Operation,
    },
    Resume {
        continuation: Continuation,
        result: ResultRows,
    },
    FlowStart {
        scope: Scope,
        mode: ReadMode,
    },
    FlowResume {
        continuation: FlowContinuation,
        result: FlowResult,
    },
    View {
        held: Option<Observation>,
        refreshed: bool,
        targets: Vec<String>,
    },
}
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    let value = match serde_json::from_slice(input).map_err(|e| e.to_string())? {
        Call::Start { operation } => serde_json::to_value(start(operation)?),
        Call::Resume {
            continuation,
            result,
        } => serde_json::to_value(resume(continuation, result)?),
        Call::FlowStart { scope, mode } => serde_json::to_value(flow_start(scope, mode)?),
        Call::FlowResume {
            continuation,
            result,
        } => serde_json::to_value(flow_resume(continuation, result)?),
        Call::View {
            held,
            refreshed,
            targets,
        } => Ok(view(held, refreshed, &targets)?),
    }
    .map_err(|e: serde_json::Error| e.to_string())?;
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> Scope {
        Scope {
            principal: "u".into(),
            lane: "canary".into(),
            audience: "a".into(),
            project: "p".into(),
        }
    }
    fn observation() -> Observation {
        let document = json!({"transformers":[{"name":"TX-1"}]});
        Observation {
            sha256: digest(&document).unwrap(),
            document,
            observed_at_ms: 42,
            needs_refresh: false,
            last_error: None,
            revision: 3,
        }
    }
    #[test]
    fn held_is_network_free_refresh_is_explicit_and_missing_is_not_empty_success() {
        for held in [None, Some(observation())] {
            let FlowStep::Storage { continuation, .. } =
                flow_start(scope(), ReadMode::Held).unwrap()
            else {
                panic!()
            };
            assert!(matches!(
                flow_resume(
                    continuation,
                    FlowResult::Storage {
                        revision: held.as_ref().map(|v| v.revision),
                        observation: held
                    }
                )
                .unwrap(),
                FlowStep::Done {
                    refreshed: false,
                    ..
                }
            ));
        }
        assert!(view(None, false, &[]).unwrap_err().contains("not_held"));
        let FlowStep::Storage { continuation, .. } =
            flow_start(scope(), ReadMode::Refresh).unwrap()
        else {
            panic!()
        };
        let FlowStep::Fetch {
            body, timeout_ms, ..
        } = flow_resume(
            continuation,
            FlowResult::Storage {
                observation: Some(observation()),
                revision: Some(3),
            },
        )
        .unwrap()
        else {
            panic!()
        };
        assert_eq!(body, request_body("p", &[]));
        assert_eq!(timeout_ms, 120000);
    }
    #[test]
    fn malformed_refresh_marks_failure_and_delayed_commit_cannot_cross_revision() {
        let step = flow_resume(
            FlowContinuation::Fetched {
                scope: scope(),
                expected: Some(3),
            },
            FlowResult::Fetch {
                document: json!({"not_transformers":[]}),
                now: 43,
            },
        )
        .unwrap();
        assert!(matches!(
            step,
            FlowStep::Storage {
                operation: Operation::MarkFailed {
                    expected: Some(3),
                    ..
                },
                ..
            }
        ));
        let continuation = Continuation::Commit {
            scope: scope(),
            document: Some(json!({"transformers":[]})),
            now: 43,
            expected: Some(3),
        };
        assert_eq!(
            resume(
                continuation,
                ResultRows {
                    rows: vec![vec![json!(4)]],
                    changes: 0
                }
            )
            .unwrap_err(),
            "status_refresh_superseded"
        );
        let continuation = Continuation::Commit {
            scope: scope(),
            document: None,
            now: 0,
            expected: Some(3),
        };
        assert!(matches!(
            resume(
                continuation,
                ResultRows {
                    rows: vec![vec![json!(4)]],
                    changes: 0
                }
            )
            .unwrap(),
            Step::Done { .. }
        ));
    }
    #[test]
    fn response_admission_preserves_document_and_rejects_missing_malformed_or_oversized_rows() {
        let original = json!({"transformers":[{"name":"TX-1","metadata":{"version":7}}],"user_email":"user@example.test"});
        assert_eq!(admit_response(original.clone()).unwrap(), original);
        assert_eq!(
            admit_response(json!({"transformers":null})).unwrap(),
            json!({"transformers":[]})
        );
        for invalid in [
            json!({}),
            json!({"transformers":{}}),
            json!({"transformers":[{"name":" bad"}]}),
            json!({"transformers":[{"name":"TX-1","kind":123}]}),
        ] {
            assert!(admit_response(invalid).is_err());
        }
        let held = observation();
        let sha = held.sha256.clone();
        let view = view(Some(held), false, &[]).unwrap();
        assert_eq!(view["observation"]["source"], "held");
        assert_eq!(view["observation"]["sha256"], sha);
    }
}
