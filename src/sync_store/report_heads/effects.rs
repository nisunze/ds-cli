//! Full-head storage protocol. One host transaction drives each operation;
//! native SQLite and browser OPFS execute precisely these bounded statements.
use super::{decode, encode, merge, validate_scope, ReportHeadObservation, MAX_BYTES};
use crate::{
    sql_effect::{ResultRows, Statement},
    sync_store::Fence,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Read {
        fence: Fence,
        project: String,
    },
    Observe {
        fence: Fence,
        project: String,
        at_ms: u64,
        heads: Vec<Value>,
    },
    Merge {
        fence: Fence,
        project: String,
        at_ms: u64,
        head: Value,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case", deny_unknown_fields)]
pub enum Continuation {
    Exists {
        fence: Fence,
        project: String,
    },
    Size {
        fence: Fence,
        project: String,
    },
    Read,
    Replace {
        bind: Vec<Value>,
    },
    Merge {
        fence: Fence,
        project: String,
        at_ms: u64,
        head: Value,
    },
    Finish,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    Sql {
        statement: Statement,
        continuation: Continuation,
    },
    Done {
        observation: Option<ReportHeadObservation>,
    },
}
fn bound_scope(fence: &Fence, project: &str, at: u64) -> Result<(), String> {
    validate_scope(fence, project, at)?;
    if at > 9_007_199_254_740_991 {
        return Err("report observation time exceeds exact SQLite effect range".into());
    }
    Ok(())
}
fn scope(fence: &Fence, project: &str) -> Vec<Value> {
    vec![
        json!(fence.account),
        json!(fence.deployment),
        json!(fence.install_id),
        json!(project),
    ]
}
fn sql(sql: &str, bind: Vec<Value>, rows: u32, bytes: u32, continuation: Continuation) -> Step {
    Step::Sql {
        statement: Statement::new(sql, bind, rows, bytes),
        continuation,
    }
}
fn read(fence: &Fence, project: &str, continuation: Continuation) -> Step {
    sql(
        super::READ,
        scope(fence, project),
        1,
        64 * 1024 * 1024,
        continuation,
    )
}
fn done(observation: Option<ReportHeadObservation>) -> Step {
    Step::Done { observation }
}
fn observation(result: &ResultRows) -> Result<Option<ReportHeadObservation>, String> {
    match result.rows.as_slice() {
        [] => Ok(None),
        [row] if row.len() == 2 => {
            let at = row[0]
                .as_u64()
                .filter(|at| *at <= 9_007_199_254_740_991)
                .ok_or("invalid report observation time")?;
            let document = row[1]
                .as_str()
                .ok_or("invalid report observation document")?;
            Ok(Some(ReportHeadObservation {
                observed_at_ms: at,
                heads: decode(document.as_bytes())?,
            }))
        }
        _ => Err("invalid report observation rows".into()),
    }
}
pub fn start(operation: Operation) -> Result<Step, String> {
    Ok(match operation {
        Operation::Read { fence, project } => {
            bound_scope(&fence, &project, 0)?;
            sql(
                super::EXISTS,
                vec![],
                1,
                32,
                Continuation::Exists { fence, project },
            )
        }
        Operation::Observe {
            fence,
            project,
            at_ms,
            heads,
        } => {
            bound_scope(&fence, &project, at_ms)?;
            let document = encode(&heads)?;
            let mut bind = scope(&fence, &project);
            bind.extend([json!(at_ms), json!(document)]);
            sql(super::SCHEMA, vec![], 0, 2, Continuation::Replace { bind })
        }
        Operation::Merge {
            fence,
            project,
            at_ms,
            head,
        } => {
            bound_scope(&fence, &project, at_ms)?;
            encode(&[head.clone()])?;
            read(
                &fence,
                &project,
                Continuation::Merge {
                    fence: fence.clone(),
                    project: project.clone(),
                    at_ms,
                    head,
                },
            )
        }
    })
}
pub fn resume(continuation: Continuation, result: ResultRows) -> Result<Step, String> {
    Ok(match continuation {
        Continuation::Exists {fence,project} => match result.rows.as_slice() {
            [row] if row.as_slice()==[json!(0)] => done(None),
            [row] if row.as_slice()==[json!(1)] => sql("SELECT length(CAST(document AS BLOB)) FROM report_head_observations WHERE account=?1 AND deployment=?2 AND install_id=?3 AND project=?4",scope(&fence,&project),1,64,Continuation::Size {fence,project}),
            _ => return Err("invalid report observation schema result".into()),
        },
        Continuation::Size {fence,project} => match result.rows.as_slice() {
            [] => done(None),
            [row] if row.len()==1 && row[0].as_u64().is_some_and(|n|n<=MAX_BYTES as u64) => read(&fence,&project,Continuation::Read),
            _ => return Err("report observation exceeds its byte bound".into()),
        },
        Continuation::Read => done(observation(&result)?),
        Continuation::Replace {bind} => {
            if !result.rows.is_empty() {return Err("unexpected report schema rows".into());}
            sql(super::REPLACE,bind,0,2,Continuation::Finish)
        },
        Continuation::Merge {fence,project,at_ms,head} => {
            let previous=observation(&result)?.ok_or("a complete report observation is required before selected merge")?;
            let document=encode(&previous.heads)?;
            if let Some(document)=merge(&document,&head)? {
                let mut bind=scope(&fence,&project);bind.extend([json!(document),json!(at_ms)]);
                sql(super::UPDATE,bind,0,2,Continuation::Finish)
            } else {done(None)}
        },
        Continuation::Finish => {
            if !result.rows.is_empty() {return Err("unexpected report write rows".into());}
            done(None)
        },
    })
}
pub fn evaluate(bytes: &[u8]) -> Result<String, String> {
    #[derive(Deserialize)]
    #[serde(tag = "call", rename_all = "snake_case", deny_unknown_fields)]
    enum Request {
        Start {
            operation: Operation,
        },
        Resume {
            continuation: Continuation,
            result: ResultRows,
        },
    }
    if bytes.len() > 128 * 1024 * 1024 {
        return Err("report store request exceeds its bound".into());
    }
    let step = match serde_json::from_slice(bytes).map_err(|e| e.to_string())? {
        Request::Start { operation } => start(operation)?,
        Request::Resume {
            continuation,
            result,
        } => resume(continuation, result)?,
    };
    serde_json::to_string(&step).map_err(|e| e.to_string())
}
