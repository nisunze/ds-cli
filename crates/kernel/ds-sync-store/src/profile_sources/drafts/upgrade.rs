//! Re-pinning a retained recovery to a newer engine. The caller first proves,
//! with that engine, that it reproduces the exact original and the exact
//! retained head; this owner then swaps only the engine pin and replaces an
//! opaque v1 checkpoint with the verified authored journal in one transaction.
//! An existing authored journal keeps every payload byte during a rebind.
use super::*;

/// The verified replacement journal: its header, opaque metadata and every row.
pub type ProfileJournalUpgrade<'a> = (&'a ProfileJournalDraftHeader, &'a [u8], &'a [&'a [u8]]);

impl Store {
    /// Upgrade one retained recovery in place. `previous` must be the stored
    /// header and `upgraded` may differ from it only in `engine`. An original
    /// without a draft is re-pinned alone (`journal: None`); an opaque v1 draft
    /// keeps its payload (`None`) or becomes the complete journal (`Some`) at the same
    /// history. An existing authored journal may only change its source engine:
    /// metadata and ordered command rows must remain identical. Stale fences or
    /// any other change refuse, leaving every record unchanged.
    pub fn upgrade_profile_recovery(
        &mut self,
        scope: &ProfileSourceScope,
        previous: &ProfileSourceHeader,
        expected_history: &Value,
        upgraded: &ProfileSourceHeader,
        journal: Option<ProfileJournalUpgrade<'_>>,
        now_ms: u64,
    ) -> Result<()> {
        let previous_raw = header_bytes(previous)?;
        let raw = header_bytes(upgraded)?;
        let mut unchanged = upgraded.clone();
        unchanged.engine = previous.engine.clone();
        if &unchanged != previous || upgraded.engine == previous.engine {
            return Err(refused(
                "Only the engine pin of an exact Profile original may be upgraded",
            ));
        }
        if let Some((header, metadata, entries)) = journal {
            journal_header_bytes(header)?;
            let entry_bytes: u64 = entries.iter().map(|entry| entry.len() as u64).sum();
            if &header.source != upgraded
                || &header.history != expected_history
                || metadata.len() as u64 != header.metadata_bytes
                || sha256_hex(metadata) != header.metadata_sha256
                || entries.len() as u64 != header.entry_count
                || entry_bytes != header.entry_bytes
                || entries
                    .iter()
                    .any(|entry| entry.is_empty() || entry.len() > MAX_PROFILE_JOURNAL_ENTRY_BYTES)
            {
                return Err(refused(
                    "Upgraded Profile journal differs from its source, history or rows",
                ));
            }
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = verify_head(&tx, scope, previous, expected_history)?;
        let preserve_rows = match (&current, journal) {
            (Some(StoredDraft::Checkpoint(_)), _) | (None, None) => false,
            (Some(StoredDraft::Journal(retained)), Some((header, metadata, entries))) => {
                let mut expected = retained.clone();
                expected.source = upgraded.clone();
                let (_, _, stored_metadata, _) = read_draft_row(&tx, scope, &previous.session_id)?
                    .ok_or_else(|| refused("Profile authored metadata disappeared"))?;
                let stored_entries =
                    read_journal_entries(&tx, scope, &previous.session_id, retained)?;
                if header != &expected
                    || metadata != stored_metadata
                    || entries.len() != stored_entries.len()
                    || entries
                        .iter()
                        .zip(&stored_entries)
                        .any(|(a, b)| *a != b.bytes)
                {
                    return Err(refused(
                        "Profile engine rebind must preserve every authored byte",
                    ));
                }
                true
            }
            _ => {
                return Err(refused(
                    "Profile engine rebind needs the exact retained recovery kind",
                ));
            }
        };
        let id = &previous.session_id;
        let count = tx.execute(
            "UPDATE profile_sources SET header=?7,header_sha256=?8,touched=?9 WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6 AND header_sha256=?10",
            params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id,raw,sha256_hex(raw.as_bytes()),now_ms,sha256_hex(previous_raw.as_bytes())])?;
        if count != 1 {
            return Err(refused(
                "Profile original changed before its engine upgrade",
            ));
        }
        if let Some((header, metadata, entries)) = journal {
            tx.execute_batch(SCHEMA)?;
            if !preserve_rows {
                delete_rows(&tx, scope, id, 0)?;
                let mut insert = tx.prepare(
                    "INSERT INTO profile_draft_entries VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                )?;
                for (ordinal, bytes) in entries.iter().enumerate() {
                    insert.execute(params![
                        scope.principal,
                        scope.lane,
                        scope.deployment,
                        scope.install,
                        scope.project,
                        id,
                        ordinal as i64,
                        sha256_hex(bytes),
                        bytes
                    ])?;
                }
            }
            let header_raw = journal_header_bytes(header)?;
            let count = if preserve_rows {
                tx.execute(
                    "UPDATE profile_drafts SET header=?7,header_sha256=?8,touched=?9 WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
                    params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id,header_raw,sha256_hex(header_raw.as_bytes()),now_ms])?
            } else {
                tx.execute(
                    "UPDATE profile_drafts SET header=?7,header_sha256=?8,checkpoint=?9,touched=?10 WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
                    params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id,header_raw,sha256_hex(header_raw.as_bytes()),metadata,now_ms])?
            };
            if count != 1 {
                return Err(refused("Profile checkpoint changed before its upgrade"));
            }
        } else if let Some(StoredDraft::Checkpoint(mut header)) = current {
            header.source = upgraded.clone();
            let header_raw = draft_header_bytes(&header)?;
            let count = tx.execute(
                "UPDATE profile_drafts SET header=?7,header_sha256=?8,touched=?9 WHERE principal=?1 AND lane=?2 AND deployment=?3 AND install_id=?4 AND project=?5 AND session_id=?6",
                params![scope.principal,scope.lane,scope.deployment,scope.install,scope.project,id,header_raw,sha256_hex(header_raw.as_bytes()),now_ms])?;
            if count != 1 {
                return Err(refused(
                    "Profile checkpoint changed before its engine rebind",
                ));
            }
        }
        tx.commit()?;
        Ok(())
    }
}
