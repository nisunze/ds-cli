//! Native SQLite effects for the shared full publication-head observation owner.
use super::*;
use ds_command_kernel::sync_store::report_heads as policy;
pub use policy::ReportHeadObservation;
use serde_json::Value;

fn drive(
    connection: &Connection,
    operation: policy::effects::Operation,
) -> Result<Option<ReportHeadObservation>> {
    use policy::effects::{self, Step};
    let mut step = effects::start(operation).map_err(Error::Kernel)?;
    for _ in 0..6 {
        step = match step {
            Step::Done { observation } => return Ok(observation),
            Step::Sql {
                statement,
                continuation,
            } => {
                let result = ds_command_kernel::sql_effect::execute(connection, &statement)
                    .map_err(Error::Kernel)?;
                effects::resume(continuation, result).map_err(Error::Kernel)?
            }
        };
    }
    Err(Error::Kernel(
        "report store exceeded its bounded continuation".into(),
    ))
}
impl Store {
    pub fn observe_report_heads(
        &mut self,
        fence: &Fence,
        project: &str,
        at_ms: u64,
        heads: &[Value],
    ) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        drive(
            &tx,
            policy::effects::Operation::Observe {
                fence: fence.clone(),
                project: project.into(),
                at_ms,
                heads: heads.to_vec(),
            },
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn observe_report_head(
        &mut self,
        fence: &Fence,
        project: &str,
        at_ms: u64,
        head: &Value,
    ) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        drive(
            &tx,
            policy::effects::Operation::Merge {
                fence: fence.clone(),
                project: project.into(),
                at_ms,
                head: head.clone(),
            },
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn report_heads(
        &self,
        fence: &Fence,
        project: &str,
    ) -> Result<Option<ReportHeadObservation>> {
        drive(
            &self.connection,
            policy::effects::Operation::Read {
                fence: fence.clone(),
                project: project.into(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fence() -> Fence {
        Fence {
            account: "owner".into(),
            deployment: "lane".into(),
            install_id: "install".into(),
        }
    }
    fn head() -> serde_json::Value {
        json!({"work_id":"work1","engine":"network_reporter","operation":"export-test","variant":"default","input_base_fingerprint":"a".repeat(64),"head_revision":1,"updated_at":"2026-10-18T00:00:00Z","outputs":[{"output_id":"xlsx","format":"xlsx","content_type":"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet","filename":ds_command_kernel::report_formats::report_filename("test","xlsx").unwrap(),"sha256":"b".repeat(64),"size_bytes":123}]})
    }
    #[test]
    fn exact_full_observation_survives_reopen_and_is_fenced() {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let path = dir.path().join("store.sqlite");
        let mut store = Store::open(&path).unwrap();
        let f = fence();
        assert!(store.report_heads(&f, "project").unwrap().is_none());
        store
            .observe_report_heads(&f, "project", 10, &[head()])
            .unwrap();
        drop(store);
        let store = Store::open_read_only(&path).unwrap().unwrap();
        let read = store.report_heads(&f, "project").unwrap().unwrap();
        assert_eq!(read.observed_at_ms, 10);
        assert_eq!(read.heads[0], head());
        let mut other = f.clone();
        other.account = "other".into();
        assert!(store.report_heads(&other, "project").unwrap().is_none());
        other = f.clone();
        other.install_id = "other".into();
        assert!(store.report_heads(&other, "project").unwrap().is_none());
        assert!(store.report_heads(&f, "other").unwrap().is_none());
    }
    #[test]
    fn selected_post_publish_readback_preserves_other_heads_and_never_regresses_revision() {
        let mut store = Store::in_memory().unwrap();
        let f = fence();
        let mut other = head();
        other["operation"] = json!("export-other");
        store
            .observe_report_heads(&f, "project", 10, &[head(), other.clone()])
            .unwrap();
        let mut next = head();
        next["head_revision"] = json!(2);
        next["work_id"] = json!("work2");
        store.observe_report_head(&f, "project", 11, &next).unwrap();
        store
            .observe_report_head(&f, "project", 12, &head())
            .unwrap();
        let observed = store.report_heads(&f, "project").unwrap().unwrap();
        assert_eq!(observed.heads, vec![next.clone(), other]);
        let mut crossed = next.clone();
        crossed["outputs"][0]["sha256"] = json!("c".repeat(64));
        assert!(store
            .observe_report_head(&f, "project", 13, &crossed)
            .is_err());
        assert_eq!(
            store.report_heads(&f, "project").unwrap().unwrap().heads[0],
            next
        );
    }

    #[test]
    fn malformed_duplicate_and_partial_observations_do_not_replace_previous_facts() {
        let mut store = Store::in_memory().unwrap();
        let f = fence();
        let h = head();
        store
            .observe_report_heads(&f, "project", 10, std::slice::from_ref(&h))
            .unwrap();
        assert!(store
            .observe_report_heads(&f, "project", 11, &[h.clone(), h])
            .is_err());
        assert!(store
            .observe_report_heads(&f, "project", 12, &[json!({"engine":"network_reporter"})])
            .is_err());
        assert_eq!(
            store
                .report_heads(&f, "project")
                .unwrap()
                .unwrap()
                .observed_at_ms,
            10
        );
        store.observe_report_heads(&f, "project", 13, &[]).unwrap();
        assert!(store
            .report_heads(&f, "project")
            .unwrap()
            .unwrap()
            .heads
            .is_empty());
    }
}
