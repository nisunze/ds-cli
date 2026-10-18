//! Full authenticated publication-head observations. Domain consumers validate
//! the selected declaration with their owner before projecting or acquiring it. These are retained read facts,
//! never another publication queue or permission to reuse engineering output.
use super::*;
use serde_json::Value;
use std::collections::BTreeSet;

const MAX_HEADS: usize = 4096;
const MAX_BYTES: usize = 32 * 1024 * 1024;
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS report_head_observations(
 account TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
 project TEXT NOT NULL, observed_at_ms INTEGER NOT NULL, document TEXT NOT NULL,
 PRIMARY KEY(account,deployment,install_id,project));";

pub struct ReportHeadObservation {
    pub observed_at_ms: u64,
    pub heads: Vec<Value>,
}
fn validated(bytes: &[u8]) -> Result<Vec<Value>> {
    if bytes.len() > MAX_BYTES {
        return Err(Error::Kernel(
            "report head observation exceeds its byte bound".into(),
        ));
    }
    let values: Vec<serde_json::Value> = serde_json::from_slice(bytes)?;
    if values.len() > MAX_HEADS {
        return Err(Error::Kernel(
            "report head observation exceeds its row bound".into(),
        ));
    }
    let mut identities = BTreeSet::new();
    values
        .into_iter()
        .map(|value| {
            let identity = ["engine", "operation", "variant"]
                .map(|key| value[key].as_str().unwrap_or_default());
            if identity
                .iter()
                .any(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
                || !identities.insert(identity.map(str::to_owned))
            {
                return Err(Error::Kernel(
                    "publication head observation contains an invalid or duplicate identity".into(),
                ));
            }
            Ok(value)
        })
        .collect()
}
impl Store {
    /// Replace only after the complete authenticated cursor walk succeeds.
    /// An empty observed inventory is distinct from never having observed it.
    pub fn observe_report_heads(
        &mut self,
        fence: &Fence,
        project: &str,
        at_ms: u64,
        heads: &[serde_json::Value],
    ) -> Result<()> {
        if [&fence.account, &fence.deployment, &fence.install_id]
            .iter()
            .any(|v| v.trim().is_empty())
        {
            return Err(Error::Kernel("incomplete report observation fence".into()));
        }
        if !ds_command_kernel::execution_context::valid_project(project) || at_ms > i64::MAX as u64
        {
            return Err(Error::Kernel(
                "report head observation scope is invalid".into(),
            ));
        }
        let bytes = serde_json::to_vec(heads)?;
        validated(&bytes)?;
        let document = String::from_utf8(bytes).map_err(|e| Error::Kernel(e.to_string()))?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(SCHEMA)?;
        tx.execute("INSERT INTO report_head_observations VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(account,deployment,install_id,project) DO UPDATE SET observed_at_ms=excluded.observed_at_ms,document=excluded.document",params![fence.account,fence.deployment,fence.install_id,project,at_ms as i64,document])?;
        tx.commit()?;
        Ok(())
    }
    /// Read back the selected publication after its ACK, without a second
    /// project walk or racing another observer's full snapshot replacement.
    pub fn observe_report_head(
        &mut self,
        fence: &Fence,
        project: &str,
        at_ms: u64,
        head: &Value,
    ) -> Result<()> {
        validated(&serde_json::to_vec(&[head])?)?;
        if at_ms > i64::MAX as u64 {
            return Err(Error::Kernel("invalid observation time".into()));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let document:String=tx.query_row("SELECT document FROM report_head_observations WHERE account=?1 AND deployment=?2 AND install_id=?3 AND project=?4",params![fence.account,fence.deployment,fence.install_id,project],|r|r.get(0))?;
        let mut heads = validated(document.as_bytes())?;
        if let Some(index) = heads.iter().position(|old| {
            ["engine", "operation", "variant"]
                .iter()
                .all(|k| old[k] == head[k])
        }) {
            let old = &heads[index];
            if old["head_revision"].as_u64() > head["head_revision"].as_u64() {
                return Ok(());
            }
            if old["head_revision"] == head["head_revision"] {
                let decode = |v: &Value| {
                    ds_command_kernel::compute_artifact_inventory::decode_head(
                        &serde_json::to_vec(v).unwrap_or_default(),
                    )
                };
                if decode(old).is_ok() && decode(old).ok() != decode(head).ok() {
                    return Err(Error::Kernel(
                        "the same publication revision has different declarations".into(),
                    ));
                }
            }
            heads[index] = head.clone();
        } else {
            heads.push(head.clone());
        }
        let bytes = serde_json::to_vec(&heads)?;
        validated(&bytes)?;
        let document = String::from_utf8(bytes).map_err(|e| Error::Kernel(e.to_string()))?;
        tx.execute("UPDATE report_head_observations SET document=?5,observed_at_ms=?6 WHERE account=?1 AND deployment=?2 AND install_id=?3 AND project=?4",params![fence.account,fence.deployment,fence.install_id,project,document,at_ms as i64])?;
        tx.commit()?;
        Ok(())
    }

    /// Read-only, including on old stores without the additive observation table.
    pub fn report_heads(
        &self,
        fence: &Fence,
        project: &str,
    ) -> Result<Option<ReportHeadObservation>> {
        let exists: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='report_head_observations')",[],|r|r.get(0))?;
        if !exists {
            return Ok(None);
        }
        let row: Option<(i64,String)> = self.connection.query_row("SELECT observed_at_ms,document FROM report_head_observations WHERE account=?1 AND deployment=?2 AND install_id=?3 AND project=?4",params![fence.account,fence.deployment,fence.install_id,project],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        row.map(|(at, document)| {
            Ok(ReportHeadObservation {
                observed_at_ms: u64::try_from(at)
                    .map_err(|_| Error::Kernel("invalid observation time".into()))?,
                heads: validated(document.as_bytes())?,
            })
        })
        .transpose()
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
            .observe_report_heads(&f, "project", 10, &[h.clone()])
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
