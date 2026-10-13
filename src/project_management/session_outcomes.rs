//! Required project-memory filing is a postcondition, never a second data mutation.
//! Hosts persist these pure transitions in the existing native sync store.
//! A prepared attempt with no verified result is unknown, never a failed or
//! completed engineering change. Recovery may file the note, never replay data.
use serde::{Deserialize, Serialize};

pub const MAX_PENDING: usize = 128;
pub const MAX_RECEIPT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub uid: String,
    pub audience_sha256: String,
    pub lane: String,
    pub project: String,
    pub install_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub scope: Scope,
    pub operation_id: String,
    pub command: String,
    pub contract: u32,
    pub intent_sha256: String,
    pub agent: Option<String>,
    pub session: Option<String>,
    pub at_ms: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Execution { Completed, Partial, Failed, Unknown }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Effect { Committed, Partial, NoChange, Unverified }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub execution: Execution,
    pub effect: Effect,
    pub receipt_sha256: Option<String>,
    pub refusal: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filing {
    pub note_id: String,
    pub version: u64,
    pub content_sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub intent: Intent,
    pub outcome: Option<Outcome>,
    pub filed: Option<Filing>,
    pub updated_at_ms: u64,
}

fn bounded(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.trim()==s && !s.chars().any(char::is_control)
}
pub fn digest(s: &str) -> bool {
    s.len()==64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Scope {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !bounded(&self.uid,256) || !digest(&self.audience_sha256)
            || !matches!(self.lane.as_str(),"stable"|"canary")
            || !crate::execution_context::valid_project(&self.project)
            || !bounded(&self.install_id,256) {
            return Err("project_note_journal_scope_invalid");
        }
        Ok(())
    }
}
impl Intent {
    pub fn validate(&self) -> Result<(), &'static str> {
        self.scope.validate()?;
        if !digest(&self.operation_id) || !digest(&self.intent_sha256)
            || !bounded(&self.command,160) || self.contract==0 || self.at_ms==0
            || self.agent.as_ref().is_some_and(|v| !bounded(v,160))
            || self.session.as_ref().is_some_and(|v| !bounded(v,256)) {
            return Err("project_note_journal_intent_invalid");
        }
        Ok(())
    }
}
impl Outcome {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.receipt_sha256.as_ref().is_some_and(|v| !digest(v))
            || self.refusal.as_ref().is_some_and(|v| !bounded(v,160))
            || (matches!(self.execution,Execution::Completed|Execution::Partial)
                && self.receipt_sha256.is_none())
            || (self.execution==Execution::Unknown
                && (self.effect!=Effect::Unverified || self.receipt_sha256.is_some()))
            || (self.execution==Execution::Failed
                && matches!(self.effect,Effect::Committed|Effect::Partial))
            || (self.execution==Execution::Completed
                && !matches!(self.effect,Effect::Committed|Effect::NoChange))
            || (self.execution==Execution::Partial && self.effect!=Effect::Partial) {
            return Err("project_note_journal_outcome_invalid");
        }
        Ok(())
    }
}
impl Filing {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !bounded(&self.note_id,256) || self.version==0 || !digest(&self.content_sha256) {
            return Err("project_note_journal_filing_invalid");
        }
        Ok(())
    }
}
pub fn prepare(intent: Intent) -> Result<Row, &'static str> {
    intent.validate()?;
    Ok(Row { updated_at_ms:intent.at_ms,intent,outcome:None,filed:None })
}
pub fn checkpoint(row: &Row, outcome: Outcome, now: u64) -> Result<Row, &'static str> {
    row.intent.validate()?;
    outcome.validate()?;
    if now < row.updated_at_ms { return Err("project_note_journal_time_invalid"); }
    if let Some(old)=&row.outcome {
        if old==&outcome { return Ok(row.clone()); }
        return Err("project_note_journal_outcome_conflict");
    }
    if row.filed.is_some() { return Err("project_note_journal_outcome_conflict"); }
    let mut next=row.clone(); next.outcome=Some(outcome); next.updated_at_ms=now; Ok(next)
}
pub fn recover_unknown(row: &Row, now: u64) -> Result<Row, &'static str> {
    if row.outcome.is_some() { return Ok(row.clone()); }
    checkpoint(row,Outcome { execution:Execution::Unknown,effect:Effect::Unverified,
        receipt_sha256:None,refusal:Some("interrupted_before_result_checkpoint".into()) },now)
}
pub fn confirm_filed(row: &Row, filing: Filing, now:u64) -> Result<Row, &'static str> {
    filing.validate()?;
    if now<row.updated_at_ms { return Err("project_note_journal_time_invalid"); }
    if row.outcome.is_none() { return Err("project_note_journal_outcome_missing"); }
    if let Some(old)=&row.filed {
        if old==&filing { return Ok(row.clone()); }
        return Err("project_note_journal_filing_conflict");
    }
    let mut next=row.clone(); next.filed=Some(filing); next.updated_at_ms=now; Ok(next)
}
pub fn scope_matches(row:&Row, scope:&Scope) -> bool { &row.intent.scope==scope }

#[cfg(test)]
mod tests {
    use super::*;
    fn intent()->Intent { Intent { scope:Scope {uid:"owner-a".into(),audience_sha256:"a".repeat(64),lane:"canary".into(),project:"project-a".into(),install_id:"install-a".into()},operation_id:"b".repeat(64),command:"design.tag.apply".into(),contract:1,intent_sha256:"c".repeat(64),agent:Some("self-reported-agent".into()),session:None,at_ms:1 } }
    fn complete()->Outcome { Outcome {execution:Execution::Completed,effect:Effect::Committed,receipt_sha256:Some("d".repeat(64)),refusal:None} }
    #[test] fn interrupted_attempt_never_becomes_success_or_data_retry() { let row=recover_unknown(&prepare(intent()).unwrap(),2).unwrap(); assert_eq!(row.outcome.as_ref().unwrap().execution,Execution::Unknown); assert_eq!(row.outcome.as_ref().unwrap().effect,Effect::Unverified); assert!(row.outcome.unwrap().receipt_sha256.is_none()); }
    #[test] fn successful_result_is_exactly_checkpointed_and_replay_is_noop() { let row=checkpoint(&prepare(intent()).unwrap(),complete(),2).unwrap(); assert_eq!(checkpoint(&row,complete(),3).unwrap(),row); assert_eq!(recover_unknown(&row,3).unwrap(),row); }
    #[test] fn completed_without_receipt_is_refused() { let mut value=complete(); value.receipt_sha256=None; assert!(checkpoint(&prepare(intent()).unwrap(),value,2).is_err()); }
    #[test] fn contradictory_failure_and_effect_are_refused() { let mut value=complete(); value.execution=Execution::Failed; assert!(value.validate().is_err()); }
    #[test] fn outcome_cannot_be_replaced_by_another_claim() { let row=checkpoint(&prepare(intent()).unwrap(),complete(),2).unwrap(); let mut other=complete(); other.receipt_sha256=Some("e".repeat(64)); assert_eq!(checkpoint(&row,other,3).unwrap_err(),"project_note_journal_outcome_conflict"); }
    #[test] fn note_confirmation_requires_outcome_and_exact_replay() { let prepared=prepare(intent()).unwrap(); let f=Filing {note_id:"note-one".into(),version:1,content_sha256:"f".repeat(64)}; assert!(confirm_filed(&prepared,f.clone(),2).is_err()); let row=confirm_filed(&checkpoint(&prepared,complete(),2).unwrap(),f.clone(),3).unwrap(); assert_eq!(confirm_filed(&row,f,4).unwrap(),row); }
    #[test] fn all_identity_boundaries_are_checked() { let row=prepare(intent()).unwrap(); let s=row.intent.scope.clone(); assert!(scope_matches(&row,&s)); for field in 0..5 { let mut other=s.clone(); match field {0=>other.uid="owner-b".into(),1=>other.lane="stable".into(),2=>other.project="project-b".into(),3=>other.install_id="install-b".into(),_=>other.audience_sha256="b".repeat(64)}; assert!(!scope_matches(&row,&other)); } }
}
