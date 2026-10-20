//! Authored Profile recovery. A v1 draft retains one opaque checkpoint produced
//! by an older Network; a v2 draft retains opaque authored metadata plus ordered,
//! digest-checked command rows, so an edit writes only its new rows. Calculated
//! results never reach this owner. The Network owner validates semantics; this
//! owner preserves exact bytes and atomically fences their retained history.
use super::*;

pub const MAX_PROFILE_CHECKPOINT_BYTES: usize = 64 * 1024 * 1024;
pub const PROFILE_JOURNAL_DRAFT_SCHEMA: &str = "ds.core.profile-draft/v2";
/// The Network journal bound; rows beyond it could never be replayed.
pub const MAX_PROFILE_JOURNAL_ENTRIES: usize = 16_384;
pub const MAX_PROFILE_JOURNAL_ENTRY_BYTES: usize = 16 * 1024 * 1024;
/// Total authored command bytes, matching the Network authored-checkpoint bound.
pub const MAX_PROFILE_JOURNAL_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_PROFILE_JOURNAL_METADATA_BYTES: usize = 1024 * 1024;
const CHECKPOINT_SCHEMA: &str = "ds.core.profile-draft/v1";
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS profile_drafts(
 principal TEXT NOT NULL, lane TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
 project TEXT NOT NULL, session_id TEXT NOT NULL, header TEXT NOT NULL, header_sha256 TEXT NOT NULL,
 checkpoint BLOB NOT NULL, touched INTEGER NOT NULL,
 PRIMARY KEY(principal,lane,deployment,install_id,project,session_id));
CREATE TABLE IF NOT EXISTS profile_draft_entries(
 principal TEXT NOT NULL, lane TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
 project TEXT NOT NULL, session_id TEXT NOT NULL, ordinal INTEGER NOT NULL CHECK(ordinal>=0),
 entry_sha256 TEXT NOT NULL, entry BLOB NOT NULL,
 PRIMARY KEY(principal,lane,deployment,install_id,project,session_id,ordinal));";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileDraftHeader {
    pub schema: String,
    pub source: ProfileSourceHeader,
    pub history: Value,
    pub checkpoint_sha256: String,
    pub checkpoint_bytes: u64,
}

/// `ds.core.profile-draft/v2`: exact five-field history, opaque producer
/// metadata and the count, size and producer chain of the ordered rows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileJournalDraftHeader {
    pub schema: String,
    pub source: ProfileSourceHeader,
    pub history: Value,
    pub entry_count: u64,
    pub entry_bytes: u64,
    /// Producer digest over the ordered rows; opaque to this owner.
    pub chain_sha256: String,
    pub metadata_sha256: String,
    pub metadata_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileJournalEntry {
    pub sha256: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileJournalDraft {
    pub header: ProfileJournalDraftHeader,
    pub metadata: Vec<u8>,
    pub entries: Vec<ProfileJournalEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileDraft {
    /// One opaque bounded checkpoint retained for an older Network engine.
    Checkpoint {
        header: ProfileDraftHeader,
        bytes: Vec<u8>,
    },
    /// Opaque authored metadata plus ordered, digest-checked command rows.
    Journal(ProfileJournalDraft),
}
impl ProfileDraft {
    pub fn history(&self) -> &Value {
        match self {
            Self::Checkpoint { header, .. } => &header.history,
            Self::Journal(draft) => &draft.header.history,
        }
    }
    pub fn source(&self) -> &ProfileSourceHeader {
        match self {
            Self::Checkpoint { header, .. } => &header.source,
            Self::Journal(draft) => &draft.header.source,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileRecovery {
    pub source: ProfileSourceHeader,
    pub source_bytes: Vec<u8>,
    pub draft: Option<ProfileDraft>,
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

fn has_journal_rows(
    connection: &rusqlite::Connection,
    scope: &ProfileSourceScope,
    id: &str,
) -> Result<bool> {
    if !has_table(connection, "profile_draft_entries")? {
        return Ok(false);
    }
    Ok(connection.query_row(
        "SELECT 1 FROM profile_draft_entries WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6 LIMIT 1",
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
fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn draft_header_bytes(header: &ProfileDraftHeader) -> Result<String> {
    header_bytes(&header.source)?;
    let raw = serde_json::to_string(header)?;
    if header.schema != CHECKPOINT_SCHEMA
        || !valid_history(&header.source.initial_history)
        || !valid_history(&header.history)
        || header.history["initial_revision"] != header.source.initial_history["initial_revision"]
        || header.checkpoint_bytes == 0
        || header.checkpoint_bytes > MAX_PROFILE_CHECKPOINT_BYTES as u64
        || !hex64(&header.checkpoint_sha256)
        || raw.len() > MAX_HEADER_BYTES
    {
        return Err(refused(
            "Profile checkpoint metadata/schema/history is invalid",
        ));
    }
    Ok(raw)
}

fn journal_header_bytes(header: &ProfileJournalDraftHeader) -> Result<String> {
    header_bytes(&header.source)?;
    let raw = serde_json::to_string(header)?;
    let depth = header.history["undo_depth"]
        .as_u64()
        .zip(header.history["redo_depth"].as_u64())
        .and_then(|(undo, redo)| undo.checked_add(redo));
    if header.schema != PROFILE_JOURNAL_DRAFT_SCHEMA
        || !valid_history(&header.source.initial_history)
        || !valid_history(&header.history)
        || header.history["initial_revision"] != header.source.initial_history["initial_revision"]
        || depth != Some(header.entry_count)
        || header.entry_count > MAX_PROFILE_JOURNAL_ENTRIES as u64
        || header.entry_bytes > MAX_PROFILE_JOURNAL_BYTES as u64
        || header.entry_bytes < header.entry_count
        || (header.entry_count == 0) != (header.entry_bytes == 0)
        || !hex64(&header.chain_sha256)
        || !hex64(&header.metadata_sha256)
        || header.metadata_bytes == 0
        || header.metadata_bytes > MAX_PROFILE_JOURNAL_METADATA_BYTES as u64
        || raw.len() > MAX_HEADER_BYTES
    {
        return Err(refused(
            "Profile authored journal metadata/schema/history is invalid",
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

/// One retained draft header, dispatched on its closed schema.
enum StoredDraft {
    Checkpoint(ProfileDraftHeader),
    Journal(ProfileJournalDraftHeader),
}
impl StoredDraft {
    fn history(&self) -> &Value {
        match self {
            Self::Checkpoint(header) => &header.history,
            Self::Journal(header) => &header.history,
        }
    }
}
#[derive(Deserialize)]
struct SchemaPeek {
    schema: String,
}
fn decode_stored_draft(
    source: &ProfileSourceHeader,
    raw: &str,
    digest: &str,
    length: i64,
) -> Result<StoredDraft> {
    if raw.len() > MAX_HEADER_BYTES || digest != sha256_hex(raw.as_bytes()) {
        return Err(refused("Profile checkpoint header digest/size changed"));
    }
    let peek: SchemaPeek = serde_json::from_str(raw)?;
    match peek.schema.as_str() {
        CHECKPOINT_SCHEMA => {
            decode_draft_header(source, raw, digest, length).map(StoredDraft::Checkpoint)
        }
        PROFILE_JOURNAL_DRAFT_SCHEMA => {
            let header: ProfileJournalDraftHeader = serde_json::from_str(raw)?;
            journal_header_bytes(&header)?;
            if &header.source != source || length != header.metadata_bytes as i64 {
                return Err(refused(
                    "Profile authored journal source/metadata size changed",
                ));
            }
            Ok(StoredDraft::Journal(header))
        }
        _ => Err(refused("Profile checkpoint schema is unsupported")),
    }
}

type DraftRow = (String, String, Vec<u8>, i64);
fn read_draft_row(
    connection: &rusqlite::Connection,
    scope: &ProfileSourceScope,
    id: &str,
) -> Result<Option<DraftRow>> {
    if !has_table(connection, "profile_drafts")? {
        return Ok(None);
    }
    Ok(connection.query_row(
        "SELECT substr(header,1,?7),substr(header_sha256,1,65),substr(checkpoint,1,?8),length(checkpoint) FROM profile_drafts WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
        params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id,MAX_HEADER_BYTES+1,MAX_PROFILE_CHECKPOINT_BYTES+1],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?)
}

/// All rows in one snapshot, bounded before allocation and checked against
/// their contiguous ordinals, digests, count and total size.
fn read_journal_entries(
    connection: &rusqlite::Connection,
    scope: &ProfileSourceScope,
    id: &str,
    header: &ProfileJournalDraftHeader,
) -> Result<Vec<ProfileJournalEntry>> {
    if !has_table(connection, "profile_draft_entries")? {
        return if header.entry_count == 0 {
            Ok(Vec::new())
        } else {
            Err(refused("Profile authored journal rows are missing"))
        };
    }
    let mut query = connection.prepare(
        "SELECT ordinal,substr(entry_sha256,1,65),substr(entry,1,?8),length(entry) FROM profile_draft_entries WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6 ORDER BY ordinal LIMIT ?7",
    )?;
    let rows = query.query_map(
        params![
            scope.principal,
            scope.lane,
            scope.deployment,
            scope.install,
            scope.project,
            id,
            header.entry_count.saturating_add(1) as i64,
            MAX_PROFILE_JOURNAL_ENTRY_BYTES + 1
        ],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, i64>(3)?,
            ))
        },
    )?;
    let mut entries = Vec::new();
    let mut total = 0u64;
    for row in rows {
        let (ordinal, digest, bytes, length) = row?;
        total = total.saturating_add(bytes.len() as u64);
        if ordinal != entries.len() as i64
            || bytes.is_empty()
            || bytes.len() > MAX_PROFILE_JOURNAL_ENTRY_BYTES
            || length != bytes.len() as i64
            || total > header.entry_bytes
            || !hex64(&digest)
            || sha256_hex(&bytes) != digest
        {
            return Err(refused("Profile authored journal row changed"));
        }
        entries.push(ProfileJournalEntry {
            sha256: digest,
            bytes,
        });
    }
    if entries.len() as u64 != header.entry_count || total != header.entry_bytes {
        return Err(refused("Profile authored journal rows are incomplete"));
    }
    Ok(entries)
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
    let draft_row = read_draft_row(connection, scope, id)?;
    let Some(source_row) = source_row else {
        return if draft_row.is_some() || has_journal_rows(connection, scope, id)? {
            Err(refused("Profile checkpoint has no matching original"))
        } else {
            Ok(None)
        };
    };
    let eligible = source_row.4;
    let (source, source_bytes) = decode_retained_source(scope, id, source_row)?;
    let draft = match (eligible, draft_row) {
        (1, None) => {
            if has_journal_rows(connection, scope, id)? {
                return Err(refused("Profile authored rows have no matching draft"));
            }
            None
        }
        (0, Some((raw, digest, bytes, length))) => {
            match decode_stored_draft(&source, &raw, &digest, length)? {
                StoredDraft::Checkpoint(header) => {
                    if bytes.len() as u64 != header.checkpoint_bytes
                        || sha256_hex(&bytes) != header.checkpoint_sha256
                    {
                        return Err(refused("Profile checkpoint source/digest/size changed"));
                    }
                    if has_journal_rows(connection, scope, id)? {
                        return Err(refused("Profile authored rows have no matching draft"));
                    }
                    Some(ProfileDraft::Checkpoint { header, bytes })
                }
                StoredDraft::Journal(header) => {
                    if bytes.len() as u64 != header.metadata_bytes
                        || sha256_hex(&bytes) != header.metadata_sha256
                    {
                        return Err(refused(
                            "Profile authored journal metadata digest/size changed",
                        ));
                    }
                    let entries = read_journal_entries(connection, scope, id, &header)?;
                    Some(ProfileDraft::Journal(ProfileJournalDraft {
                        header,
                        metadata: bytes,
                        entries,
                    }))
                }
            }
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
        .map_or(&recovery.source.initial_history, ProfileDraft::history);
    if &recovery.source != source || history != expected_history {
        return Err(refused("Profile recovery source/history changed"));
    }
    Ok(recovery)
}

/// Header-only compare-and-swap fence for journal retention. It neither reads
/// the original package nor any retained row, so an edit stays O(change).
fn verify_head(
    connection: &rusqlite::Connection,
    scope: &ProfileSourceScope,
    source: &ProfileSourceHeader,
    expected_history: &Value,
) -> Result<Option<StoredDraft>> {
    if !valid_scope(scope) || &source.scope != scope || !valid_history(expected_history) {
        return Err(refused("Invalid Profile recovery scope/history"));
    }
    header_bytes(source)?;
    let id = &source.session_id;
    let row: Option<(String, String, i64, i64)> = if has_table(connection, "profile_sources")? {
        connection.query_row(
            "SELECT substr(header,1,?7),substr(header_sha256,1,65),length(package),eligible FROM profile_sources WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
            params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id,MAX_HEADER_BYTES+1],
            |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?
    } else {
        None
    };
    let (raw, digest, length, eligible) =
        row.ok_or_else(|| refused("Profile recovery no longer exists"))?;
    let stored = decode_source_header(scope, id, &raw, &digest, length, eligible)?;
    if &stored != source {
        return Err(refused("Profile recovery source/history changed"));
    }
    let draft_row: Option<(String, String, i64)> = if has_table(connection, "profile_drafts")? {
        connection.query_row(
            "SELECT substr(header,1,?7),substr(header_sha256,1,65),length(checkpoint) FROM profile_drafts WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
            params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id,MAX_HEADER_BYTES+1],
            |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?
    } else {
        None
    };
    let draft = match (eligible, draft_row) {
        (1, None) => None,
        (0, Some((raw, digest, length))) => {
            Some(decode_stored_draft(source, &raw, &digest, length)?)
        }
        _ => {
            return Err(refused(
                "Profile original/checkpoint eligibility is inconsistent",
            ))
        }
    };
    if !matches!(draft, Some(StoredDraft::Journal(_))) && has_journal_rows(connection, scope, id)? {
        return Err(refused("Profile authored rows have no matching draft"));
    }
    let history = draft
        .as_ref()
        .map_or(&source.initial_history, StoredDraft::history);
    if history != expected_history {
        return Err(refused("Profile recovery source/history changed"));
    }
    Ok(draft)
}

fn delete_rows(
    connection: &rusqlite::Connection,
    scope: &ProfileSourceScope,
    id: &str,
    from: u64,
) -> Result<()> {
    if has_table(connection, "profile_draft_entries")? {
        connection.execute(
            "DELETE FROM profile_draft_entries WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6 AND ordinal>=?7",
            params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id,from as i64])?;
    }
    Ok(())
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
        if !valid_scope(scope) || !hex64(package_sha256) {
            return Err(refused("Invalid Profile recovery scope/package digest"));
        }
        let tx = self.connection.unchecked_transaction()?;
        let mut ids = std::collections::BTreeSet::new();
        let has_sources = has_table(&tx, "profile_sources")?;
        let has_drafts = has_table(&tx, "profile_drafts")?;
        let has_rows = has_table(&tx, "profile_draft_entries")?;
        for (table, exists) in [
            ("profile_sources", has_sources),
            ("profile_drafts", has_drafts),
            ("profile_draft_entries", has_rows),
        ] {
            if !exists {
                continue;
            }
            let mut query = tx.prepare(&format!(
                "SELECT DISTINCT substr(session_id,1,33) FROM {table} WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 ORDER BY session_id LIMIT 9"))?;
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
                decode_stored_draft(&source, &raw, &digest, length)?;
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
        delete_rows(&tx, scope, &source.session_id, 0)?;
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

    /// Retain an authored journal draft as ordered rows. `keep` leading rows of
    /// the current journal draft stay untouched; every later row is replaced by
    /// `appended`. Compare-and-swap binds the exact source, expected history
    /// and, for an existing journal, its chain. The new header must account for
    /// exactly `keep + appended` rows. The producer validated their semantics;
    /// any failed fence or write leaves every record unchanged.
    #[allow(clippy::too_many_arguments)]
    pub fn retain_profile_journal_draft(
        &mut self,
        scope: &ProfileSourceScope,
        source: &ProfileSourceHeader,
        expected_history: &Value,
        expected_chain: Option<&str>,
        header: &ProfileJournalDraftHeader,
        metadata: &[u8],
        keep: u64,
        appended: &[&[u8]],
        now_ms: u64,
    ) -> Result<()> {
        let raw = journal_header_bytes(header)?;
        if &header.source != source
            || metadata.len() as u64 != header.metadata_bytes
            || sha256_hex(metadata) != header.metadata_sha256
            || keep.checked_add(appended.len() as u64) != Some(header.entry_count)
            || appended
                .iter()
                .any(|bytes| bytes.is_empty() || bytes.len() > MAX_PROFILE_JOURNAL_ENTRY_BYTES)
        {
            return Err(refused(
                "Profile authored journal source/metadata/rows changed",
            ));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = verify_head(&tx, scope, source, expected_history)?;
        match (&current, expected_chain) {
            (Some(StoredDraft::Journal(current)), Some(chain))
                if current.chain_sha256 == chain && keep <= current.entry_count => {}
            (None | Some(StoredDraft::Checkpoint(_)), None) if keep == 0 => {}
            _ => {
                return Err(refused(
                    "Profile authored journal prefix changed before retention",
                ))
            }
        }
        tx.execute_batch(SCHEMA)?;
        let id = &source.session_id;
        delete_rows(&tx, scope, id, keep)?;
        {
            let mut insert =
                tx.prepare("INSERT INTO profile_draft_entries VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)")?;
            for (offset, bytes) in appended.iter().enumerate() {
                insert.execute(params![
                    scope.principal,
                    scope.lane,
                    scope.deployment,
                    scope.install,
                    scope.project,
                    id,
                    (keep + offset as u64) as i64,
                    sha256_hex(bytes),
                    bytes
                ])?;
            }
        }
        let (count, total): (i64, i64) = tx.query_row(
            "SELECT count(*),coalesce(sum(length(entry)),0) FROM profile_draft_entries WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
            params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id],
            |r| Ok((r.get(0)?, r.get(1)?)))?;
        if count != header.entry_count as i64 || total != header.entry_bytes as i64 {
            return Err(refused(
                "Profile authored journal rows differ from their header",
            ));
        }
        let count = tx.execute(
            "UPDATE profile_sources SET eligible=0,touched=?7 WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
            params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id,now_ms])?;
        if count != 1 {
            return Err(refused("Profile original changed before journal retention"));
        }
        let count = tx.execute(
            "INSERT INTO profile_drafts VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(principal,lane,deployment,install_id,project,session_id)
             DO UPDATE SET header=excluded.header,header_sha256=excluded.header_sha256,checkpoint=excluded.checkpoint,touched=excluded.touched",
            params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id,raw,sha256_hex(raw.as_bytes()),metadata,now_ms])?;
        if count != 1 {
            return Err(refused("Profile authored journal was not retained"));
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
            delete_rows(&tx, scope, &source.session_id, 0)?;
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

mod upgrade;

#[cfg(test)]
mod journal_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod upgrade_tests;
