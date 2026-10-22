//! Durable handoff to the authenticated native sender. No URLs or credentials.
use crate::Workspace;
use anyhow::{Result, ensure};
use ds_solar_contracts::{SolarCityInput, digest};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

pub fn snapshot(input: &SolarCityInput) -> Value {
    json!({"_root":input.seeded.root,"_config":input.seeded.config,"00_templates":input.seeded.templates,"01_city_inputs":input.seeded.city_inputs})
}
pub fn fingerprint(input: &SolarCityInput) -> Result<String> {
    ds_command_kernel::solar_snapshot::input_base::fingerprint(&snapshot(input))
        .map_err(anyhow::Error::msg)
}
impl Workspace {
    /// The sender consumes the oldest row. A failed city commit must not be
    /// skipped to publish a calculation against unrelated cloud inputs.
    pub fn sync_next(&self) -> Result<Value> {
        self.sync_next_scope(false)
    }
    pub fn sync_next_scope(&self, inputs_only: bool) -> Result<Value> {
        self.sync_next_selected(inputs_only, None)
    }
    pub fn sync_next_selected(&self, inputs_only: bool, run_id: Option<&str>) -> Result<Value> {
        let Some(row) = self.pending_selected(1, inputs_only, run_id)?.pop() else {
            return Ok(json!({"pending":false}));
        };
        let jobs = match row.kind.as_str() {
            "city" => {
                let input = crate::records::Record::decode(row.payload.clone())?;
                ensure!(
                    input.digest() == row.digest
                        && input.identity().city_id == row.identity
                        && input.identity().project_id == self.identity.project_id
                        && input.identity().root == self.identity.root,
                    "pending city identity mismatch"
                );
                let key = format!("remote:{}", row.identity);
                let previous: Option<String> = self
                    .db
                    .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
                    .optional()?;
                let initial = input
                    .initial_revision()
                    .filter(|s| {
                        s.len() == 64
                            && s.bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    })
                    .unwrap_or("");
                vec![
                    json!({"operation":"commit_city","city":row.identity,"expected_base":previous.as_deref().unwrap_or(initial),"snapshot_json":String::from_utf8(digest::canonical_json_bytes(&input.snapshot())?)?,"fingerprint":digest::hex_sha256(&digest::canonical_json_bytes(&input.snapshot())?)}),
                ]
            }
            "run" => anyhow::bail!(
                "legacy_publication_discard_required: preview and discard old queued publications, then recompute through the native application"
            ),
            _ => anyhow::bail!("unknown Solar publication kind"),
        };
        Ok(
            json!({"pending":true,"sequence":row.sequence,"digest":row.digest,"kind":row.kind,"identity":row.identity,"jobs":jobs}),
        )
    }

    /// Explicit operator conflict resolution; never an automatic overwrite.
    pub fn rebase_pending_city(&mut self, sequence: i64, expected_cloud: &str) -> Result<()> {
        ensure!(
            expected_cloud.len() == 64
                && expected_cloud
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "expected cloud fingerprint must be exact"
        );
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (current,kind,city):(i64,String,String)=tx.query_row("SELECT sequence,kind,identity FROM outbox WHERE status='pending' AND kind='city' ORDER BY sequence LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        ensure!(
            current == sequence && kind == "city",
            "rebase must name the oldest pending city revision"
        );
        tx.execute(
            "INSERT INTO meta VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![format!("remote:{city}"), expected_cloud],
        )?;
        tx.execute("UPDATE outbox SET error=NULL WHERE sequence=?1", [sequence])?;
        tx.commit()?;
        Ok(())
    }

    pub fn sync_ack(&mut self, sequence: i64, digest: &str, receipt: &Value) -> Result<()> {
        self.sync_ack_scope(sequence, digest, receipt, false)
    }
    pub fn sync_ack_scope(
        &mut self,
        sequence: i64,
        digest: &str,
        receipt: &Value,
        inputs_only: bool,
    ) -> Result<()> {
        self.sync_ack_selected(sequence, digest, receipt, inputs_only, None)
    }
    pub fn sync_ack_selected(
        &mut self,
        sequence: i64,
        digest: &str,
        receipt: &Value,
        inputs_only: bool,
        run_id: Option<&str>,
    ) -> Result<()> {
        let row = self
            .pending_selected(1, inputs_only, run_id)?
            .pop()
            .ok_or_else(|| anyhow::anyhow!("no pending upload"))?;
        ensure!(
            row.sequence == sequence && row.digest == digest,
            "Solar acknowledgement is not the current upload"
        );
        let encoded = serde_json::to_string(receipt)?;
        ensure!(encoded.len() <= 1024 * 1024, "receipt exceeds 1 MiB");
        let next = if row.kind == "city" {
            let input = crate::records::Record::decode(row.payload)?;
            ensure!(
                input.digest() == row.digest
                    && input.identity().city_id == row.identity
                    && input.identity().project_id == self.identity.project_id
                    && input.identity().root == self.identity.root,
                "pending city identity mismatch"
            );
            let expected = digest::hex_sha256(&digest::canonical_json_bytes(&input.snapshot())?);
            ensure!(
                receipt["client_snapshot_sha256"] == expected
                    && receipt["city_id"] == row.identity
                    && receipt["root"] == self.identity.root,
                "city upload receipt mismatch"
            );
            let base = receipt["input_base_fingerprint"]
                .as_str()
                .filter(|s| {
                    s.len() == 64
                        && s.bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
                .ok_or_else(|| anyhow::anyhow!("invalid cloud fingerprint"))?;
            Some(base.to_string())
        } else {
            anyhow::bail!(
                "legacy_publication_discard_required: old run publications must be discarded and recomputed natively"
            );
        };
        let tx = self.db.transaction()?;
        if let Some(base) = next {
            tx.execute("INSERT INTO meta VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![format!("remote:{}",row.identity),base])?;
        }
        ensure!(tx.execute("UPDATE outbox SET status='published',receipt=?1,error=NULL WHERE sequence=?2 AND digest=?3 AND status='pending'",params![encoded,sequence,digest])?==1,"pending upload moved");
        tx.commit()?;
        Ok(())
    }
}
