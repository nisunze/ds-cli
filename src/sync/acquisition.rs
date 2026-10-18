//! Bounded publication acquisition shared by native Sync and browser effects.
//! The caller holds its existing project lease and checks its authority fence
//! around every effect. A completed walk alone may replace held observations.
use crate::{compute_artifact_inventory as inventory, sync_store::report_heads};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadWalk {
    project: String,
    heads: Vec<Value>,
    cursors: BTreeSet<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HeadStep {
    Gateway { body: Value, continuation: HeadWalk },
    Done { heads: Vec<Value> },
}
impl HeadWalk {
    pub fn start(project: &str) -> Result<HeadStep, String> {
        if !crate::execution_context::valid_project(project) {
            return Err("invalid publication project".into());
        }
        Ok(HeadStep::Gateway {
            body: json!({"action":"list","project_id":project,"limit":25}),
            continuation: Self {
                project: project.into(),
                heads: vec![],
                cursors: BTreeSet::new(),
            },
        })
    }
    pub fn resume(mut self, page: Value) -> Result<HeadStep, String> {
        let heads = page
            .get("heads")
            .and_then(Value::as_array)
            .ok_or("the record did not return a head inventory")?;
        let cursor = match page.get("next_cursor") {
            None | Some(Value::Null) => "",
            Some(Value::String(s)) => s,
            _ => return Err("invalid head cursor".into()),
        };
        if cursor.len() > 4096 {
            return Err("head cursor exceeds its bound".into());
        }
        self.heads.extend(heads.iter().cloned());
        // Admission retains the full document and detects repeated identities;
        // a failed/truncated walk never produces a replaceable observation.
        report_heads::encode(&self.heads)?;
        if !cursor.is_empty() && self.heads.len() == report_heads::MAX_HEADS {
            return Err("the complete head inventory exceeds its bound".into());
        }
        if cursor.is_empty() {
            return Ok(HeadStep::Done { heads: self.heads });
        }
        if heads.is_empty() || !self.cursors.insert(cursor.into()) {
            return Err("head pagination did not advance".into());
        }
        Ok(HeadStep::Gateway {
            body: json!({"action":"list","project_id":self.project,"limit":25,"cursor":cursor}),
            continuation: self,
        })
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum TicketError {
    HeadMoved,
    Unreadable(String),
}
pub fn download_body(
    project: &str,
    head: &inventory::Head,
    output_id: &str,
) -> Result<Value, String> {
    if !crate::execution_context::valid_project(project)
        || !head.outputs.iter().any(|o| o.output_id == output_id)
    {
        return Err("invalid publication output selector".into());
    }
    Ok(
        json!({"action":"download","project_id":project,"engine":head.engine,"operation":head.operation,"variant":head.variant,"output_id":output_id}),
    )
}
/// Exact revision, full declaration/provenance and selected output are admitted
/// before a host receives the object ticket. Historical filenames use the one
/// declaration normalizer, never a second native/browser projection.
pub fn admitted_ticket_url<'a>(
    head: &inventory::Head,
    expected: &inventory::Declaration,
    ticket: &'a Value,
) -> Result<&'a str, TicketError> {
    let unreadable = TicketError::Unreadable;
    let resolved = inventory::decode_head(
        &serde_json::to_vec(&ticket["head"]).map_err(|e| unreadable(e.to_string()))?,
    )
    .map_err(unreadable)?;
    if resolved != *head {
        return Err(TicketError::HeadMoved);
    }
    let projection=inventory::evaluate(&serde_json::to_vec(&json!({"engine":head.engine,"operation":head.operation,"outputs":head.outputs,"download":{"output":ticket["output"],"file_name":ticket["file_name"]}})).map_err(|e|unreadable(e.to_string()))?).map_err(unreadable)?;
    #[derive(Deserialize)]
    struct Projection {
        download_output: inventory::Declaration,
    }
    let projection: Projection =
        serde_json::from_str(&projection).map_err(|e| unreadable(e.to_string()))?;
    if projection.download_output != *expected {
        return Err(TicketError::HeadMoved);
    }
    ticket["download_url"]
        .as_str()
        .ok_or_else(|| unreadable("The selected output has no governed download ticket.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn head(op: &str) -> Value {
        json!({"engine":"network_reporter","operation":op,"variant":"default","head_revision":1})
    }
    #[test]
    fn complete_walk_once_and_partial_failures_never_return_observations() {
        let HeadStep::Gateway { body, continuation } = HeadWalk::start("project").unwrap() else {
            panic!()
        };
        assert_eq!(body["limit"], 25);
        let HeadStep::Gateway { body, continuation } = continuation
            .resume(json!({"heads":[head("export-a")],"next_cursor":"next"}))
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(body["cursor"], "next");
        assert!(continuation
            .clone()
            .resume(json!({"heads":[],"next_cursor":"other"}))
            .is_err());
        assert!(continuation
            .clone()
            .resume(json!({"heads":[head("export-b")],"next_cursor":"next"}))
            .is_err());
        assert!(continuation
            .clone()
            .resume(json!({"heads":[head("export-a")]}))
            .is_err());
        let HeadStep::Done { heads } = continuation
            .resume(json!({"heads":[head("export-b")]}))
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(heads.len(), 2);
    }
}
