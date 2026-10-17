//! Opaque authored checkpoints. The Network owner validates checkpoint semantics;
//! this owner preserves exact bytes and atomically fences their retained history.
use super::*;

pub const MAX_PROFILE_CHECKPOINT_BYTES: usize = 64 * 1024 * 1024;
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS profile_drafts(
 principal TEXT NOT NULL, lane TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
 project TEXT NOT NULL, session_id TEXT NOT NULL, header TEXT NOT NULL, header_sha256 TEXT NOT NULL,
 checkpoint BLOB NOT NULL, touched INTEGER NOT NULL,
 PRIMARY KEY(principal,lane,deployment,install_id,project,session_id));";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileDraftHeader {
    pub schema: String,
    pub source: ProfileSourceHeader,
    pub history: Value,
    pub checkpoint_sha256: String,
    pub checkpoint_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileRecovery {
    pub source: ProfileSourceHeader,
    pub source_bytes: Vec<u8>,
    pub draft: Option<(ProfileDraftHeader, Vec<u8>)>,
}

fn has_table(connection: &rusqlite::Connection, name: &str) -> Result<bool> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

pub(super) fn has_draft(
    connection: &rusqlite::Connection,
    scope: &ProfileSourceScope,
    id: &str,
) -> Result<bool> {
    if !has_table(connection, "profile_drafts")? {
        return Ok(false);
    }
    Ok(connection.query_row(
        "SELECT 1 FROM profile_drafts WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
        params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id],
        |_|Ok(())).optional()?.is_some())
}

// No model/checkpoint interpretation here. The exact five-field receipt remains
// opaque except for its shape and the immutable original's initial revision.
fn valid_history(history: &Value) -> bool {
    history.as_object().is_some_and(|object| object.len() == 5)
        && ["model_revision", "initial_revision", "history_pin"]
            .iter()
            .all(|key| {
                history[*key]
                    .as_str()
                    .is_some_and(|value| !value.is_empty())
            })
        && history["undo_depth"].as_u64().is_some()
        && history["redo_depth"].as_u64().is_some()
}

fn draft_header_bytes(header: &ProfileDraftHeader) -> Result<String> {
    header_bytes(&header.source)?;
    let raw = serde_json::to_string(header)?;
    if header.schema != "ds.core.profile-draft/v1"
        || !valid_history(&header.source.initial_history)
        || !valid_history(&header.history)
        || header.history["initial_revision"] != header.source.initial_history["initial_revision"]
        || header.checkpoint_bytes == 0
        || header.checkpoint_bytes > MAX_PROFILE_CHECKPOINT_BYTES as u64
        || header.checkpoint_sha256.len() != 64
        || !header
            .checkpoint_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || raw.len() > MAX_HEADER_BYTES
    {
        return Err(refused(
            "Profile checkpoint metadata/schema/history is invalid",
        ));
    }
    Ok(raw)
}

fn decode_draft_header(
    source: &ProfileSourceHeader,
    raw: &str,
    digest: &str,
    length: i64,
) -> Result<ProfileDraftHeader> {
    if raw.len() > MAX_HEADER_BYTES || digest != sha256_hex(raw.as_bytes()) {
        return Err(refused("Profile checkpoint header digest/size changed"));
    }
    let header: ProfileDraftHeader = serde_json::from_str(raw)?;
    draft_header_bytes(&header)?;
    if &header.source != source || length != header.checkpoint_bytes as i64 {
        return Err(refused("Profile checkpoint source/digest/size changed"));
    }
    Ok(header)
}

fn read_recovery(
    connection: &rusqlite::Connection,
    scope: &ProfileSourceScope,
    id: &str,
) -> Result<Option<ProfileRecovery>> {
    let source_row = if has_table(connection, "profile_sources")? {
        read_source_row(connection, scope, id)?
    } else {
        None
    };
    let draft_row: Option<(String, String, Vec<u8>, i64)> = if has_table(
        connection,
        "profile_drafts",
    )? {
        connection.query_row(
            "SELECT substr(header,1,?7),substr(header_sha256,1,65),substr(checkpoint,1,?8),length(checkpoint) FROM profile_drafts WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
            params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id,MAX_HEADER_BYTES+1,MAX_PROFILE_CHECKPOINT_BYTES+1],
            |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?
    } else {
        None
    };
    let Some(source_row) = source_row else {
        return if draft_row.is_some() {
            Err(refused("Profile checkpoint has no matching original"))
        } else {
            Ok(None)
        };
    };
    let eligible = source_row.4;
    let (source, source_bytes) = decode_retained_source(scope, id, source_row)?;
    let draft = match (eligible, draft_row) {
        (1, None) => None,
        (0, Some((raw, digest, bytes, length))) => {
            let header = decode_draft_header(&source, &raw, &digest, length)?;
            if bytes.len() as u64 != header.checkpoint_bytes
                || sha256_hex(&bytes) != header.checkpoint_sha256
            {
                return Err(refused("Profile checkpoint source/digest/size changed"));
            }
            Some((header, bytes))
        }
        _ => {
            return Err(refused(
                "Profile original/checkpoint eligibility is inconsistent",
            ))
        }
    };
    Ok(Some(ProfileRecovery {
        source,
        source_bytes,
        draft,
    }))
}

fn verify_recovery(
    connection: &rusqlite::Connection,
    scope: &ProfileSourceScope,
    source: &ProfileSourceHeader,
    expected_history: &Value,
) -> Result<ProfileRecovery> {
    if !valid_scope(scope) || &source.scope != scope || !valid_history(expected_history) {
        return Err(refused("Invalid Profile recovery scope/history"));
    }
    header_bytes(source)?;
    let recovery = read_recovery(connection, scope, &source.session_id)?
        .ok_or_else(|| refused("Profile recovery no longer exists"))?;
    let history = recovery
        .draft
        .as_ref()
        .map_or(&recovery.source.initial_history, |(header, _)| {
            &header.history
        });
    if &recovery.source != source || history != expected_history {
        return Err(refused("Profile recovery source/history changed"));
    }
    Ok(recovery)
}

impl Store {
    /// Read original and checkpoint in one SQLite snapshot. An ineligible
    /// original is recoverable only together with its exact valid checkpoint.
    /// Absence/read does not create tables, touch recency, or acquire a lease.
    pub fn profile_recovery(
        &self,
        scope: &ProfileSourceScope,
        id: &str,
    ) -> Result<Option<ProfileRecovery>> {
        if !valid_scope(scope) {
            return Err(refused("Invalid Profile recovery scope"));
        }
        let tx = self.connection.unchecked_transaction()?;
        let recovery = read_recovery(&tx, scope, id)?;
        tx.commit()?;
        Ok(recovery)
    }

    /// Discover a unique authored draft for the exact original package in this
    /// scope. Valid readonly originals do not claim a draft. Ambiguous, corrupt
    /// or orphaned evidence refuses instead of permitting a fresh-open fallback.
    pub fn profile_draft_for_package(
        &self,
        scope: &ProfileSourceScope,
        package_sha256: &str,
    ) -> Result<Option<ProfileRecovery>> {
        if !valid_scope(scope)
            || package_sha256.len() != 64
            || !package_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(refused("Invalid Profile recovery scope/package digest"));
        }
        let tx = self.connection.unchecked_transaction()?;
        let mut ids = std::collections::BTreeSet::new();
        let has_sources = has_table(&tx, "profile_sources")?;
        let has_drafts = has_table(&tx, "profile_drafts")?;
        for (table, exists) in [
            ("profile_sources", has_sources),
            ("profile_drafts", has_drafts),
        ] {
            if !exists {
                continue;
            }
            let mut query = tx.prepare(&format!(
                "SELECT substr(session_id,1,33) FROM {table} WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 ORDER BY session_id LIMIT 9"))?;
            let rows = query.query_map(
                params![
                    scope.principal,
                    scope.lane,
                    scope.deployment,
                    scope.install,
                    scope.project
                ],
                |r| r.get::<_, String>(0),
            )?;
            for row in rows {
                ids.insert(row?);
            }
            if ids.len() > MAX_PROFILE_SOURCES {
                return Err(refused("Profile recovery exceeds its logical capacity"));
            }
        }
        let mut found = None;
        for id in ids {
            let source_row: Option<(String, String, i64, i64)> = if has_sources {
                tx.query_row(
                    "SELECT substr(header,1,?7),substr(header_sha256,1,65),length(package),eligible FROM profile_sources WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
                    params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id,MAX_HEADER_BYTES+1],
                    |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?
            } else {
                None
            };
            let Some((raw, digest, length, eligible)) = source_row else {
                return Err(refused("Profile checkpoint has no matching original"));
            };
            let source = decode_source_header(scope, &id, &raw, &digest, length, eligible)?;
            let draft_row: Option<(String, String, i64)> = if has_drafts {
                tx.query_row(
                    "SELECT substr(header,1,?7),substr(header_sha256,1,65),length(checkpoint) FROM profile_drafts WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
                    params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id,MAX_HEADER_BYTES+1],
                    |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?
            } else {
                None
            };
            if let Some((raw, digest, length)) = draft_row {
                decode_draft_header(&source, &raw, &digest, length)?;
                if eligible != 0 {
                    return Err(refused(
                        "Profile original/checkpoint eligibility is inconsistent",
                    ));
                }
            }
            if source.package_sha256 == package_sha256 {
                let recovery = read_recovery(&tx, scope, &id)?
                    .ok_or_else(|| refused("Profile recovery no longer exists"))?;
                if recovery.draft.is_some() {
                    if found.is_some() {
                        return Err(refused("Profile package has ambiguous retained drafts"));
                    }
                    found = Some(recovery);
                }
            }
        }
        tx.commit()?;
        Ok(found)
    }

    /// Replace only the exact retained history. The caller validates checkpoint
    /// semantics with the captured Network owner before retaining opaque bytes.
    /// First retention requires an eligible original and its initial history;
    /// an earlier original-only invalidation cannot be repaired by this API.
    pub fn retain_profile_draft(
        &mut self,
        scope: &ProfileSourceScope,
        source: &ProfileSourceHeader,
        expected_history: &Value,
        header: &ProfileDraftHeader,
        bytes: &[u8],
        now_ms: u64,
    ) -> Result<()> {
        let raw = draft_header_bytes(header)?;
        if &header.source != source
            || bytes.len() as u64 != header.checkpoint_bytes
            || sha256_hex(bytes) != header.checkpoint_sha256
        {
            return Err(refused("Profile checkpoint source/digest/size changed"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        verify_recovery(&tx, scope, source, expected_history)?;
        tx.execute_batch(SCHEMA)?;
        let count = tx.execute(
            "UPDATE profile_sources SET eligible=0,touched=?7 WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
            params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,source.session_id,now_ms])?;
        if count != 1 {
            return Err(refused(
                "Profile original changed before checkpoint retention",
            ));
        }
        let count = tx.execute(
            "INSERT INTO profile_drafts VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(principal,lane,deployment,install_id,project,session_id)
             DO UPDATE SET header=excluded.header,header_sha256=excluded.header_sha256,checkpoint=excluded.checkpoint,touched=excluded.touched",
            params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,source.session_id,raw,sha256_hex(raw.as_bytes()),bytes,now_ms])?;
        if count != 1 {
            return Err(refused("Profile checkpoint was not retained"));
        }
        tx.commit()?;
        Ok(())
    }

    /// Explicit close/discard after the caller's authority checks. Both records
    /// are deleted atomically only for the exact source and retained history.
    pub fn remove_profile_recovery(
        &mut self,
        scope: &ProfileSourceScope,
        source: &ProfileSourceHeader,
        expected_history: &Value,
    ) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let recovery = verify_recovery(&tx, scope, source, expected_history)?;
        if recovery.draft.is_some() {
            let count = tx.execute(
                "DELETE FROM profile_drafts WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
                params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,source.session_id])?;
            if count != 1 {
                return Err(refused("Profile checkpoint changed before removal"));
            }
        }
        let count = tx.execute(
            "DELETE FROM profile_sources WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
            params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,source.session_id])?;
        if count != 1 {
            return Err(refused("Profile original changed before recovery removal"));
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
