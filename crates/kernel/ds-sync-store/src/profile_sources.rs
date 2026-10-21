//! Bounded immutable Profile opening sources, in the existing protected Store.
//! Originals and opaque authored checkpoints, never publication or compute jobs.
use crate::{Error, Result, Store};
use ds_command_kernel::report_export::sha256_hex;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod drafts;
pub use drafts::{
    ProfileDraft, ProfileDraftHeader, ProfileJournalDraft, ProfileJournalDraftHeader,
    ProfileJournalEntry, ProfileRecovery, MAX_PROFILE_CHECKPOINT_BYTES, MAX_PROFILE_JOURNAL_BYTES,
    MAX_PROFILE_JOURNAL_ENTRIES, MAX_PROFILE_JOURNAL_ENTRY_BYTES,
    MAX_PROFILE_JOURNAL_METADATA_BYTES, PROFILE_JOURNAL_DRAFT_SCHEMA,
};

pub const MAX_PROFILE_SOURCE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PROFILE_SOURCES: usize = 8;
const MAX_HEADER_BYTES: usize = 64 * 1024;
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS profile_sources(
 principal TEXT NOT NULL, lane TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
 project TEXT NOT NULL, session_id TEXT NOT NULL, header TEXT NOT NULL, header_sha256 TEXT NOT NULL,
 package BLOB NOT NULL, eligible INTEGER NOT NULL CHECK(eligible IN (0,1)), touched INTEGER NOT NULL,
 PRIMARY KEY(principal,lane,deployment,install_id,project,session_id));";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSourceScope {
    pub principal: String,
    pub lane: String,
    pub deployment: String,
    pub install: String,
    pub project: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSourceHeader {
    pub scope: ProfileSourceScope,
    pub schema: String,
    pub session_id: String,
    pub model_id: String,
    pub package_sha256: String,
    pub package_bytes: u64,
    pub initial_history: Value,
    pub source_revision: u64,
    pub coordinate_system: String,
    pub engine: Value,
}
fn refused(message: &str) -> Error {
    Error::Kernel(message.into())
}
fn valid_scope(scope: &ProfileSourceScope) -> bool {
    matches!(scope.lane.as_str(), "stable" | "canary")
        && [
            &scope.principal,
            &scope.deployment,
            &scope.install,
            &scope.project,
        ]
        .iter()
        .all(|s| {
            !s.is_empty()
                && s.len() <= 2048
                && s.trim() == s.as_str()
                && !s.chars().any(char::is_control)
        })
}
fn initial(history: &Value) -> bool {
    history["model_revision"]
        .as_str()
        .is_some_and(|s| !s.is_empty())
        && history["model_revision"] == history["initial_revision"]
        && history["undo_depth"] == 0
        && history["redo_depth"] == 0
        && history["history_pin"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
}
fn header_bytes(header: &ProfileSourceHeader) -> Result<String> {
    let bytes = serde_json::to_string(header)?;
    if !valid_scope(&header.scope)
        || header.schema != "ds.core.profile-source/v1"
        || header.session_id.len() != 32
        || !header
            .session_id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || header.model_id.is_empty()
        || header.model_id.len() > 2048
        || header.coordinate_system.is_empty()
        || header.coordinate_system.len() > 2048
        || header.package_bytes == 0
        || header.package_bytes > MAX_PROFILE_SOURCE_BYTES as u64
        || header.package_sha256.len() != 64
        || !header
            .package_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || !initial(&header.initial_history)
        || !header.engine.is_object()
        || bytes.len() > MAX_HEADER_BYTES
    {
        return Err(refused(
            "Profile original metadata/schema/initial history is invalid",
        ));
    }
    Ok(bytes)
}
type SourceRow = (String, String, Vec<u8>, i64, i64);
fn read_source_row(
    connection: &rusqlite::Connection,
    scope: &ProfileSourceScope,
    id: &str,
) -> Result<Option<SourceRow>> {
    Ok(connection.query_row(
        "SELECT substr(header,1,?7),substr(header_sha256,1,65),substr(package,1,?8),length(package),eligible FROM profile_sources WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
        params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id,MAX_HEADER_BYTES+1,MAX_PROFILE_SOURCE_BYTES+1],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?)
}
fn decode_source(
    scope: &ProfileSourceScope,
    id: &str,
    row: SourceRow,
) -> Result<(ProfileSourceHeader, Vec<u8>)> {
    if row.4 != 1 {
        return Err(refused("Profile original is no longer readonly"));
    }
    decode_retained_source(scope, id, row)
}
fn decode_retained_source(
    scope: &ProfileSourceScope,
    id: &str,
    row: SourceRow,
) -> Result<(ProfileSourceHeader, Vec<u8>)> {
    let (raw, digest, bytes, length, eligible) = row;
    let header = decode_source_header(scope, id, &raw, &digest, length, eligible)?;
    if bytes.len() as u64 != header.package_bytes || sha256_hex(&bytes) != header.package_sha256 {
        return Err(refused("Profile original package digest/size changed"));
    }
    Ok((header, bytes))
}
fn decode_source_header(
    scope: &ProfileSourceScope,
    id: &str,
    raw: &str,
    digest: &str,
    length: i64,
    eligible: i64,
) -> Result<ProfileSourceHeader> {
    if raw.len() > MAX_HEADER_BYTES
        || digest != sha256_hex(raw.as_bytes())
        || !matches!(eligible, 0 | 1)
    {
        return Err(refused(
            "Profile original is corrupt, incompatible or no longer readonly",
        ));
    }
    let header: ProfileSourceHeader = serde_json::from_str(raw)?;
    header_bytes(&header)?;
    if &header.scope != scope || header.session_id != id || length != header.package_bytes as i64 {
        return Err(refused("Profile original package digest/size changed"));
    }
    Ok(header)
}

fn verify_current_source(
    connection: &rusqlite::Connection,
    scope: &ProfileSourceScope,
    expected: &ProfileSourceHeader,
    expected_raw: &str,
) -> Result<()> {
    if drafts::has_draft(connection, scope, &expected.session_id)? {
        return Err(refused("Profile original has an authored checkpoint"));
    }
    let row = read_source_row(connection, scope, &expected.session_id)?;
    let Some(row) = row else {
        return Err(refused("Profile original changed during native admission"));
    };
    if row.0 != expected_raw {
        return Err(refused(
            "Profile original header changed during native admission",
        ));
    }
    let (header, _) = decode_source(scope, &expected.session_id, row)?;
    if &header != expected {
        return Err(refused("Profile original changed during native admission"));
    }
    Ok(())
}
impl Store {
    fn has_profile_sources(&self) -> Result<bool> {
        Ok(self
            .connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='profile_sources'",
                [],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
    /// Keep at most eight originals for the canonical owner, across projects.
    /// Only dormant eligible rows may be evicted, on this intentional Open.
    pub fn retain_profile_source(
        &mut self,
        scope: &ProfileSourceScope,
        header: &ProfileSourceHeader,
        bytes: &[u8],
        active: &[String],
        now_ms: u64,
    ) -> Result<()> {
        if !valid_scope(scope) || active.len() > MAX_PROFILE_SOURCES {
            return Err(refused("Invalid Profile source scope/capacity"));
        }
        if &header.scope != scope {
            return Err(refused(
                "Profile original metadata belongs to another scope",
            ));
        }
        let raw = header_bytes(header)?;
        if bytes.len() as u64 != header.package_bytes || sha256_hex(bytes) != header.package_sha256
        {
            return Err(refused("Profile original package digest/size changed"));
        }
        self.connection.execute_batch(SCHEMA)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if drafts::has_draft(&tx, scope, &header.session_id)? {
            return Err(refused(
                "Immutable Profile original has an authored checkpoint",
            ));
        }
        let existing: Option<(String,String,i64)> = tx.query_row(
            "SELECT substr(header,1,65537),substr(header_sha256,1,65),eligible FROM profile_sources WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
            params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,header.session_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        if let Some((previous, digest, eligible)) = existing {
            if previous != raw || digest != sha256_hex(raw.as_bytes()) || eligible != 1 {
                return Err(refused("Immutable Profile original cannot be replaced"));
            }
            // Replay verifies actual BLOB too; a corrupt source is never blessed.
            let blob: Vec<u8> = tx.query_row("SELECT substr(package,1,?7) FROM profile_sources WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
                params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,header.session_id,MAX_PROFILE_SOURCE_BYTES+1],|r|r.get(0))?;
            if blob != bytes {
                return Err(refused("Immutable Profile original bytes changed"));
            }
            tx.commit()?;
            return Ok(());
        }
        let rows: Vec<(String, String, i64)> = {
            let mut query = tx.prepare("SELECT substr(project,1,2049),substr(session_id,1,33),eligible FROM profile_sources WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 ORDER BY touched,session_id LIMIT 9")?;
            let rows = query
                .query_map(
                    params![scope.principal, scope.lane, scope.deployment, scope.install],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?
                .collect::<std::result::Result<_, _>>()?;
            rows
        };
        if rows.len() > MAX_PROFILE_SOURCES {
            return Err(refused("Profile source cache exceeds its logical capacity"));
        }
        if rows.len() == MAX_PROFILE_SOURCES {
            let mut candidate = None;
            for (project, id, eligible) in &rows {
                let mut candidate_scope = scope.clone();
                candidate_scope.project = project.clone();
                if *eligible == 1
                    && !active.contains(id)
                    && !drafts::has_draft(&tx, &candidate_scope, id)?
                {
                    candidate = Some((project, id));
                    break;
                }
            }
            let Some((project, id)) = candidate else {
                return Err(refused(
                    "Profile source capacity holds active or non-recoverable history",
                ));
            };
            // Never evict corrupt or future-schema recovery evidence.
            let evicted: (String,String,Vec<u8>,i64,i64) = tx.query_row(
                "SELECT substr(header,1,?7),substr(header_sha256,1,65),substr(package,1,?8),length(package),eligible FROM profile_sources WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
                params![scope.principal,scope.lane,scope.deployment,scope.install,project,id,MAX_HEADER_BYTES+1,MAX_PROFILE_SOURCE_BYTES+1],
                |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
            let mut evicted_scope = scope.clone();
            evicted_scope.project = project.clone();
            decode_source(&evicted_scope, id, evicted)?;
            tx.execute("DELETE FROM profile_sources WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6 AND eligible=1",
                params![scope.principal,scope.lane,scope.deployment,scope.install,project,id])?;
        }
        tx.execute(
            "INSERT INTO profile_sources VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,1,?10)",
            params![
                scope.principal,
                scope.lane,
                scope.deployment,
                scope.install,
                scope.project,
                header.session_id,
                raw,
                sha256_hex(raw.as_bytes()),
                bytes,
                now_ms
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// No table creation, lease or touch occurs on absence/read.
    pub fn profile_source(
        &self,
        scope: &ProfileSourceScope,
        id: &str,
    ) -> Result<Option<(ProfileSourceHeader, Vec<u8>)>> {
        if !valid_scope(scope) {
            return Err(refused("Invalid Profile source scope"));
        }
        if !self.has_profile_sources()? {
            return Ok(None);
        }
        let tx = self.connection.unchecked_transaction()?;
        if drafts::has_draft(&tx, scope, id)? {
            return Err(refused("Profile original has an authored checkpoint"));
        }
        let result = read_source_row(&tx, scope, id)?
            .map(|row| decode_source(scope, id, row))
            .transpose()?;
        tx.commit()?;
        Ok(result)
    }
    /// Recheck immutable metadata after external launch, then touch. Never hold
    /// an SQLite writer transaction while a process is running an operation.
    pub fn touch_profile_source(
        &mut self,
        scope: &ProfileSourceScope,
        header: &ProfileSourceHeader,
        now_ms: u64,
    ) -> Result<()> {
        if !valid_scope(scope) || &header.scope != scope {
            return Err(refused("Invalid Profile source scope"));
        }
        let raw = header_bytes(header)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        verify_current_source(&tx, scope, header, &raw)?;
        let count=tx.execute("UPDATE profile_sources SET touched=?7 WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
            params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,header.session_id,now_ms])?;
        if count != 1 {
            return Err(refused("Profile original changed during native admission"));
        }
        tx.commit()?;
        Ok(())
    }
    /// Must precede any future authored mutation, even if its reply is lost.
    pub fn invalidate_profile_source(
        &mut self,
        scope: &ProfileSourceScope,
        id: &str,
    ) -> Result<()> {
        if !valid_scope(scope) {
            return Err(refused("Invalid Profile source scope"));
        }
        if self.has_profile_sources()? {
            self.connection.execute("UPDATE profile_sources SET eligible=0 WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id])?;
        }
        Ok(())
    }
    /// Caller already admitted exact source metadata/history and late authority.
    pub fn remove_profile_source(
        &mut self,
        scope: &ProfileSourceScope,
        header: &ProfileSourceHeader,
    ) -> Result<()> {
        if !valid_scope(scope) || &header.scope != scope {
            return Err(refused("Invalid Profile source scope"));
        }
        let raw = header_bytes(header)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        verify_current_source(&tx, scope, header, &raw)?;
        let count=tx.execute("DELETE FROM profile_sources WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6 AND header_sha256=?7 AND header=?8 AND eligible=1",
            params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,header.session_id,sha256_hex(raw.as_bytes()),raw])?;
        if count != 1 {
            return Err(refused("Profile original changed before close"));
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> ProfileSourceScope {
        ProfileSourceScope {
            principal: "owner".into(),
            lane: "canary".into(),
            deployment: "fixture".into(),
            install: "install".into(),
            project: "project".into(),
        }
    }
    fn header(n: u8) -> ProfileSourceHeader {
        ProfileSourceHeader {
            scope: scope(),
            schema: "ds.core.profile-source/v1".into(),
            session_id: format!("{n:032x}"),
            model_id: "model".into(),
            package_sha256: sha256_hex(b"exact synthetic source"),
            package_bytes: 22,
            initial_history: serde_json::json!({"model_revision":"rev","initial_revision":"rev","undo_depth":0,"redo_depth":0,"history_pin":"pin"}),
            source_revision: 1,
            coordinate_system: "EPSG:3395".into(),
            engine: serde_json::json!({"fixture":true}),
        }
    }
    fn retain(store: &mut Store, n: u8, active: &[String]) -> Result<()> {
        store.retain_profile_source(
            &scope(),
            &header(n),
            b"exact synthetic source",
            active,
            n as u64,
        )
    }
    #[test]
    fn exact_replay_scope_close_and_no_queue_changes() {
        let mut s = Store::in_memory().unwrap();
        retain(&mut s, 1, &[]).unwrap();
        retain(&mut s, 1, &[]).unwrap();
        let (h, b) = s
            .profile_source(&scope(), &header(1).session_id)
            .unwrap()
            .unwrap();
        assert_eq!(b, b"exact synthetic source");
        for field in 0..5 {
            let mut x = scope();
            match field {
                0 => x.principal = "other".into(),
                1 => x.lane = "stable".into(),
                2 => x.deployment = "other".into(),
                3 => x.install = "other".into(),
                _ => x.project = "other".into(),
            };
            assert!(s.profile_source(&x, &h.session_id).unwrap().is_none());
        }
        s.touch_profile_source(&scope(), &h, 10).unwrap();
        s.remove_profile_source(&scope(), &h).unwrap();
        assert!(s.profile_source(&scope(), &h.session_id).unwrap().is_none());
        for table in ["artifacts", "heads", "receipts", "leases"] {
            assert_eq!(
                s.connection
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }
    #[test]
    fn bounded_dormant_eviction_never_replaces_active_or_ineligible() {
        let mut s = Store::in_memory().unwrap();
        for n in 1..=8 {
            retain(&mut s, n, &[]).unwrap();
        }
        let active: Vec<_> = (1..=8).map(|n| header(n).session_id).collect();
        assert!(retain(&mut s, 9, &active).is_err());
        s.invalidate_profile_source(&scope(), &header(1).session_id)
            .unwrap();
        retain(&mut s, 9, &active[2..]).unwrap();
        assert!(s
            .profile_source(&scope(), &header(2).session_id)
            .unwrap()
            .is_none());
        assert!(s.profile_source(&scope(), &header(1).session_id).is_err());
        assert!(s
            .profile_source(&scope(), &header(9).session_id)
            .unwrap()
            .is_some());
    }
    #[test]
    fn malformed_history_digest_schema_and_corrupt_blob_refuse() {
        let mut s = Store::in_memory().unwrap();
        for field in 0..4 {
            let mut h = header(1);
            match field {
                0 => h.initial_history["undo_depth"] = 1.into(),
                1 => h.schema = "future".into(),
                2 => h.package_sha256 = "0".repeat(64),
                _ => h.package_bytes = MAX_PROFILE_SOURCE_BYTES as u64 + 1,
            };
            assert!(s
                .retain_profile_source(&scope(), &h, b"exact synthetic source", &[], 0)
                .is_err());
        }
        retain(&mut s, 1, &[]).unwrap();
        s.connection
            .execute("UPDATE profile_sources SET package=x'00'", [])
            .unwrap();
        assert!(s.profile_source(&scope(), &header(1).session_id).is_err());
        assert!(s.touch_profile_source(&scope(), &header(1), 5).is_err());
        assert!(retain(&mut s, 1, &[]).is_err());
    }
    #[test]
    fn late_invalidation_refuses_touch_and_preserves_source() {
        let mut s = Store::in_memory().unwrap();
        retain(&mut s, 1, &[]).unwrap();
        let h = header(1);
        s.invalidate_profile_source(&scope(), &h.session_id)
            .unwrap();
        assert!(s.touch_profile_source(&scope(), &h, 2).is_err());
        assert!(s.profile_source(&scope(), &h.session_id).is_err());
    }
    #[test]
    fn auxiliary_schema_two_reopens_without_changing_queue_or_legacy_schema() {
        let temp = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let path = temp.path().join("store.sqlite");
        {
            let mut s = Store::open(&path).unwrap();
            retain(&mut s, 1, &[]).unwrap();
        }
        // Store::open and global schema2 reader are unchanged by this extension.
        let s = Store::open(&path).unwrap();
        assert_eq!(
            super::super::schema_version(&s.connection).unwrap(),
            Some(2)
        );
        assert!(s
            .profile_source(&scope(), &header(1).session_id)
            .unwrap()
            .is_some());
        assert!(Store::open_read_only(&path).unwrap().is_some());
    }
    #[test]
    fn incompatible_or_corrupt_dormant_row_is_not_evicted() {
        let mut s = Store::in_memory().unwrap();
        for n in 1..=8 {
            retain(&mut s, n, &[]).unwrap();
        }
        let mut future = header(1);
        future.schema = "future".into();
        let raw = serde_json::to_string(&future).unwrap();
        s.connection
            .execute(
                "UPDATE profile_sources SET header=?1,header_sha256=?2 WHERE session_id=?3",
                params![raw, sha256_hex(raw.as_bytes()), future.session_id],
            )
            .unwrap();
        assert!(s.profile_source(&scope(), &future.session_id).is_err());
        assert!(retain(&mut s, 9, &[]).is_err());
        assert_eq!(
            s.connection
                .query_row("SELECT count(*) FROM profile_sources", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            8
        );
        assert!(s
            .profile_source(&scope(), &header(9).session_id)
            .unwrap()
            .is_none());
    }
    #[test]
    fn late_header_or_blob_tamper_refuses_touch_and_close_without_erasing_evidence() {
        for blob in [false, true] {
            let mut s = Store::in_memory().unwrap();
            retain(&mut s, 1, &[]).unwrap();
            let (h, _) = s
                .profile_source(&scope(), &header(1).session_id)
                .unwrap()
                .unwrap();
            if blob {
                s.connection
                    .execute("UPDATE profile_sources SET package=x'01'", [])
                    .unwrap();
            } else {
                s.connection
                    .execute("UPDATE profile_sources SET header='{}'", [])
                    .unwrap();
            }
            assert!(s.touch_profile_source(&scope(), &h, 99).is_err());
            assert!(s.remove_profile_source(&scope(), &h).is_err());
            assert_eq!(
                s.connection
                    .query_row("SELECT count(*) FROM profile_sources", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
    }
}
