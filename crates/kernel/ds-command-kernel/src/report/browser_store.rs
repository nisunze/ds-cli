//! Browser identity and the existing Sync lease table in the common SQLite DB.
//! No network runs within these transactions. Lease decisions remain sync_store::apply.
use crate::{
    sql_effect::{ResultRows, Statement},
    sync_store::{self as store, Event, Fence, LeaseRow, RowChange, Scope, Snapshot, Transition},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
const ID_SCHEMA:&str="CREATE TABLE IF NOT EXISTS browser_identity(singleton INTEGER PRIMARY KEY CHECK(singleton=1), install_id TEXT NOT NULL)";
const LEASE_SCHEMA:&str="CREATE TABLE IF NOT EXISTS leases(account TEXT NOT NULL,deployment TEXT NOT NULL,install_id TEXT NOT NULL,scope TEXT NOT NULL,row TEXT NOT NULL,PRIMARY KEY(account,deployment,install_id,scope))";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Identity {
        candidate: String,
    },
    Lease {
        fence: Fence,
        project: String,
        worker: String,
        now: u64,
        lease_action: LeaseAction,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseAction {
    Take,
    Renew,
    Release,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case", deny_unknown_fields)]
pub enum Continuation {
    IdentityInsert {
        candidate: String,
    },
    IdentityRead,
    IdentityDone,
    LeaseRead {
        fence: Fence,
        project: String,
        worker: String,
        now: u64,
        lease_action: LeaseAction,
    },
    LeaseApply {
        fence: Fence,
        project: String,
        worker: String,
        now: u64,
        lease_action: LeaseAction,
    },
    LeaseDone {
        transition: Transition,
    },
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    Sql {
        statement: Statement,
        continuation: Continuation,
    },
    Done {
        install_id: Option<String>,
        transition: Option<Transition>,
    },
}
fn sql(
    statement: &str,
    bind: Vec<Value>,
    rows: u32,
    bytes: u32,
    continuation: Continuation,
) -> Step {
    Step::Sql {
        statement: Statement::new(statement, bind, rows, bytes),
        continuation,
    }
}
fn identity(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
            }
        })
}
fn bind(fence: &Fence, project: &str) -> Vec<Value> {
    vec![
        json!(fence.account),
        json!(fence.deployment),
        json!(fence.install_id),
        json!(format!("project:{project}")),
    ]
}
pub fn start(operation: Operation) -> Result<Step, String> {
    Ok(match operation {
        Operation::Identity { candidate } => {
            if !identity(&candidate) {
                return Err("invalid browser installation identity".into());
            }
            sql(
                ID_SCHEMA,
                vec![],
                0,
                2,
                Continuation::IdentityInsert { candidate },
            )
        }
        Operation::Lease {
            fence,
            project,
            worker,
            now,
            lease_action,
        } => {
            store::report_heads::validate_scope(&fence, &project, now)?;
            if worker.is_empty() || worker.len() > 128 || now > 9_007_199_254_740_991 {
                return Err("invalid report lease scope".into());
            }
            sql(
                LEASE_SCHEMA,
                vec![],
                0,
                2,
                Continuation::LeaseRead {
                    fence,
                    project,
                    worker,
                    now,
                    lease_action,
                },
            )
        }
    })
}
pub fn resume(continuation: Continuation, result: ResultRows) -> Result<Step, String> {
    Ok(match continuation {
        Continuation::IdentityInsert{candidate}=>sql("INSERT INTO browser_identity(singleton,install_id) VALUES(1,?1) ON CONFLICT(singleton) DO NOTHING",vec![json!(candidate)],0,2,Continuation::IdentityRead),
        Continuation::IdentityRead=>sql("SELECT install_id FROM browser_identity WHERE singleton=1",vec![],1,128,Continuation::IdentityDone),
        Continuation::IdentityDone=>{
            let [row]=result.rows.as_slice() else{return Err("invalid browser installation row".into())};
            let [value]=row.as_slice() else{return Err("invalid browser installation value".into())};
            let value=value.as_str().filter(|v|identity(v)).ok_or("invalid browser installation identity")?;
            Step::Done{install_id:Some(value.into()),transition:None}
        },
        Continuation::LeaseRead{fence,project,worker,now,lease_action}=>sql("SELECT row FROM leases WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4",bind(&fence,&project),1,4096,Continuation::LeaseApply{fence,project,worker,now,lease_action}),
        Continuation::LeaseApply{fence,project,worker,now,lease_action}=> {
            let lease:Option<LeaseRow>=match result.rows.as_slice(){[]=>None,[row]if row.len()==1=>Some(serde_json::from_str(row[0].as_str().ok_or("invalid report lease row")?).map_err(|e|format!("invalid report lease: {e}"))?),_=>return Err("invalid report lease rows".into())};
            let scope=Scope::Project{project:project.clone()};
            let event=match lease_action{LeaseAction::Take=>Event::LeaseTake{scope,worker_id:worker,ttl_ms:300_000},LeaseAction::Renew=>Event::LeaseRenew{scope,worker_id:worker,ttl_ms:300_000},LeaseAction::Release=>Event::LeaseRelease{scope,worker_id:worker}};
            let transition=store::apply(&store::Request{schema:store::SCHEMA.into(),fence:fence.clone(),now_ms:now,snapshot:Snapshot{artifacts:vec![],heads:vec![],grants:vec![],lease},event})?;
            match transition.changes.as_slice(){
                []=>Step::Done{install_id:None,transition:Some(transition)},
                [RowChange::SetLease(lease)]=> {
                    let mut args=bind(&fence,&project);args.push(json!(serde_json::to_string(lease).map_err(|e|e.to_string())?));
                    sql("INSERT INTO leases(account,deployment,install_id,scope,row) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(account,deployment,install_id,scope) DO UPDATE SET row=excluded.row",args,0,2,Continuation::LeaseDone{transition})
                },
                [RowChange::ClearLease{..}]=>sql("DELETE FROM leases WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4",bind(&fence,&project),0,2,Continuation::LeaseDone{transition}),
                _=>return Err("unexpected report lease transition".into()),
            }
        },
        Continuation::LeaseDone{transition}=>{
            if !result.rows.is_empty(){return Err("unexpected lease write rows".into());}
            Step::Done{install_id:None,transition:Some(transition)}
        },
    })
}

pub fn evaluate(bytes: &[u8]) -> Result<String, String> {
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
    }
    let step = match serde_json::from_slice(bytes).map_err(|e| e.to_string())? {
        Call::Start { operation } => start(operation)?,
        Call::Resume {
            continuation,
            result,
        } => resume(continuation, result)?,
    };
    serde_json::to_string(&step).map_err(|e| e.to_string())
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name=reportBrowserStore)]
pub fn wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    evaluate(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}
