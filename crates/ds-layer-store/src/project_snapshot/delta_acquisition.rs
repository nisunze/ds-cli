//! Durable metadata journal; this is deliberately not a download cache.
use super::*;
use policy::{DeltaPage, Revision};

const MAX_PENDING_BATCHES: usize = 8;
const MAX_PAGES: usize = 256;
const MAX_PAGE_BYTES: usize = 1024 * 1024;
const MAX_BATCH_BYTES: usize = 16 * 1024 * 1024;
const MAX_PENDING_BYTES: usize = 32 * 1024 * 1024;
const MAX_RECEIPTS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeltaAcquisitionState {
    Acquiring,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeltaAcquisition {
    pub batch_id: String,
    pub base_manifest_sha256: String,
    pub target: Revision,
    pub pages: u64,
    pub serialized_bytes: u64,
    pub next_page: Option<u64>,
    pub terminal_manifest_sha256: Option<String>,
    pub state: DeltaAcquisitionState,
}

fn invalid(reason: &str) -> Refusal {
    refusal(RefusalCode::SnapshotInvalid, reason)
}
fn budget(reason: &str) -> Refusal {
    refusal(RefusalCode::SnapshotBudgetExceeded, reason)
}
fn stale(reason: &str) -> Refusal {
    refusal(RefusalCode::SnapshotStaleDelta, reason)
}

pub(super) fn schema(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS delta_pending(batch TEXT PRIMARY KEY,base TEXT NOT NULL,header TEXT NOT NULL,target_revision TEXT NOT NULL,next_page INTEGER,page_count INTEGER NOT NULL,bytes INTEGER NOT NULL,terminal_sha TEXT);
CREATE TABLE IF NOT EXISTS delta_pending_pages(batch TEXT NOT NULL,page INTEGER NOT NULL,digest TEXT NOT NULL,body TEXT NOT NULL,PRIMARY KEY(batch,page));
CREATE TABLE IF NOT EXISTS delta_receipts(seq INTEGER PRIMARY KEY AUTOINCREMENT,batch TEXT UNIQUE NOT NULL,base TEXT NOT NULL,target TEXT NOT NULL,header TEXT NOT NULL,page_count INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS delta_receipt_pages(batch TEXT NOT NULL,page INTEGER NOT NULL,digest TEXT NOT NULL,PRIMARY KEY(batch,page));").map_err(storage)
}

// Refuse oversized serialization without first allocating its full body.
struct Count(usize);
impl Write for Count {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        if self.0 > MAX_PAGE_BYTES {
            return Err(std::io::Error::other("delta page exceeds 1 MiB"));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encoded(page: &DeltaPage) -> Result<(String, String)> {
    serde_json::to_writer(&mut Count(0), page)
        .map_err(|_| budget("delta page exceeds 1 MiB or is not serializable"))?;
    let body = serde_json::to_string(page).map_err(storage)?;
    let digest = canonical_hash(b"ds.snapshot-delta-page/v1\0", page)?;
    Ok((body, digest))
}
fn header(page: &DeltaPage) -> Result<String> {
    canonical_hash(
        b"ds.snapshot-delta-batch/v1\0",
        &(
            &page.context,
            &page.batch_id,
            &page.base_manifest_sha256,
            &page.from,
            &page.target,
        ),
    )
}

pub(super) struct Completed {
    batch: String,
    base: String,
    target: String,
    header: String,
    digests: Vec<String>,
}
impl Completed {
    pub(super) fn from_pages(store: &Store, pages: &[DeltaPage]) -> Result<Self> {
        if pages.is_empty() || pages.len() > MAX_PAGES {
            return Err(invalid("delta batch requires 1..=256 pages"));
        }
        let first = &pages[0];
        // Size admission precedes hashing an untrusted header.
        encoded(first)?;
        let batch_header = header(first)?;
        let mut bytes = 0usize;
        let mut digests = Vec::new();
        for (i, page) in pages.iter().enumerate() {
            let (body, digest) = encoded(page)?;
            bytes = bytes.saturating_add(body.len());
            if bytes > MAX_BATCH_BYTES {
                return Err(budget("delta batch exceeds 16 MiB"));
            }
            if page.context != store.context
                || header(page)? != batch_header
                || page.page != i as u64
                || page.next_page
                    != if i + 1 < pages.len() {
                        Some(i as u64 + 1)
                    } else {
                        None
                    }
                || page.terminal_manifest.is_some() != (i + 1 == pages.len())
            {
                return Err(stale(
                    "delta batch context, identity or ordered page chain differs",
                ));
            }
            digests.push(digest);
        }
        let terminal = pages
            .last()
            .and_then(|p| p.terminal_manifest.as_ref())
            .ok_or_else(|| invalid("delta batch is incomplete"))?;
        terminal.validate(&store.context)?;
        terminal.verify_sha256()?;
        if terminal.context != store.context
            || terminal.source_kind != store.source
            || terminal.revision != first.target
        {
            return Err(stale("delta terminal authority differs"));
        }
        Ok(Self {
            batch: first.batch_id.clone(),
            base: first.base_manifest_sha256.clone(),
            target: terminal.manifest_sha256.clone(),
            header: batch_header,
            digests,
        })
    }
}

fn noop(target: String) -> Admission {
    Admission {
        manifest_sha256: target,
        changed: false,
        imported_chunks: 0,
        reused_chunks: 0,
        imported_bytes: 0,
        verified_reused_bytes: 0,
        elapsed_ms: 0,
    }
}

pub(super) fn replay(c: &Connection, expected: &Completed) -> Result<Option<Admission>> {
    let saved: Option<(String, String, String, u64)> = c
        .query_row(
            "SELECT base,target,header,page_count FROM delta_receipts WHERE batch=?1",
            [&expected.batch],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(storage)?;
    let Some((base, target, header, count)) = saved else {
        return Ok(None);
    };
    if base != expected.base
        || target != expected.target
        || header != expected.header
        || count != expected.digests.len() as u64
        || Store::head_digest(c)?.as_deref() != Some(target.as_str())
    {
        return Err(stale(
            "completed delta replay differs or its target is no longer head",
        ));
    }
    let mut statement = c
        .prepare("SELECT page,digest FROM delta_receipt_pages WHERE batch=?1 ORDER BY page")
        .map_err(storage)?;
    let rows = statement
        .query_map([&expected.batch], |r| {
            Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(storage)?;
    let mut seen = 0usize;
    for row in rows {
        let (page, digest) = row.map_err(storage)?;
        if page != seen as u64 || expected.digests.get(seen) != Some(&digest) {
            return Err(stale("completed delta page replay differs"));
        }
        seen += 1;
    }
    if seen != expected.digests.len() {
        return Err(storage("completed delta page receipts are incomplete"));
    }
    Ok(Some(noop(target)))
}

pub(super) fn check_pending(c: &Connection, expected: &Completed) -> Result<()> {
    let saved: Option<(String, String)> = c
        .query_row(
            "SELECT base,header FROM delta_pending WHERE batch=?1",
            [&expected.batch],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    if let Some((base, header)) = saved {
        if base != expected.base || header != expected.header {
            return Err(stale("pending delta batch identity differs"));
        }
        let mut statement = c
            .prepare("SELECT page,digest FROM delta_pending_pages WHERE batch=?1 ORDER BY page")
            .map_err(storage)?;
        let rows = statement
            .query_map([&expected.batch], |r| {
                Ok((r.get::<_, usize>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(storage)?;
        for row in rows {
            let (page, digest) = row.map_err(storage)?;
            if expected.digests.get(page) != Some(&digest) {
                return Err(stale("pending delta page differs"));
            }
        }
    }
    Ok(())
}

pub(super) fn commit(c: &Connection, completed: &Completed) -> Result<()> {
    c.execute(
        "INSERT INTO delta_receipts(batch,base,target,header,page_count) VALUES(?1,?2,?3,?4,?5)",
        params![
            completed.batch,
            completed.base,
            completed.target,
            completed.header,
            completed.digests.len()
        ],
    )
    .map_err(storage)?;
    for (page, digest) in completed.digests.iter().enumerate() {
        c.execute(
            "INSERT INTO delta_receipt_pages(batch,page,digest) VALUES(?1,?2,?3)",
            params![completed.batch, page, digest],
        )
        .map_err(storage)?;
    }
    delete_pending(c, &completed.batch)?;
    c.execute("DELETE FROM delta_receipt_pages WHERE batch IN (SELECT batch FROM delta_receipts ORDER BY seq DESC LIMIT -1 OFFSET ?1)",[MAX_RECEIPTS]).map_err(storage)?;
    c.execute("DELETE FROM delta_receipts WHERE seq NOT IN (SELECT seq FROM delta_receipts ORDER BY seq DESC LIMIT ?1)",[MAX_RECEIPTS]).map_err(storage)?;
    Ok(())
}
fn delete_pending(c: &Connection, batch: &str) -> Result<bool> {
    c.execute("DELETE FROM delta_pending_pages WHERE batch=?1", [batch])
        .map_err(storage)?;
    Ok(
        c.execute("DELETE FROM delta_pending WHERE batch=?1", [batch])
            .map_err(storage)?
            != 0,
    )
}
pub(super) fn pending_bases(c: &Connection) -> Result<BTreeSet<String>> {
    let mut s = c
        .prepare("SELECT DISTINCT base FROM delta_pending")
        .map_err(storage)?;
    let rows = s
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(storage)?;
    rows.map(|r| r.map_err(storage)).collect()
}
fn status(c: &Connection, batch: &str) -> Result<Option<DeltaAcquisition>> {
    type PendingRow = (String, String, u64, u64, Option<u64>, Option<String>);
    let row:Option<PendingRow>=c.query_row("SELECT base,target_revision,page_count,bytes,next_page,terminal_sha FROM delta_pending WHERE batch=?1",[batch],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(storage)?;
    row.map(|(base, target, pages, bytes, next, terminal)| {
        Ok(DeltaAcquisition {
            batch_id: batch.into(),
            base_manifest_sha256: base,
            target: serde_json::from_str(&target).map_err(storage)?,
            pages,
            serialized_bytes: bytes,
            next_page: next,
            state: if terminal.is_some() {
                DeltaAcquisitionState::Ready
            } else {
                DeltaAcquisitionState::Acquiring
            },
            terminal_manifest_sha256: terminal,
        })
    })
    .transpose()
}
fn load(c: &Connection, batch: &str) -> Result<Vec<DeltaPage>> {
    let mut s = c
        .prepare("SELECT page,digest,body FROM delta_pending_pages WHERE batch=?1 ORDER BY page")
        .map_err(storage)?;
    let rows = s
        .query_map([batch], |r| {
            Ok((
                r.get::<_, usize>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(storage)?;
    let mut result = Vec::new();
    let mut bytes = 0usize;
    for row in rows {
        let (index, digest, body) = row.map_err(storage)?;
        bytes = bytes.saturating_add(body.len());
        if index != result.len()
            || result.len() >= MAX_PAGES
            || body.len() > MAX_PAGE_BYTES
            || bytes > MAX_BATCH_BYTES
        {
            return Err(storage("invalid pending page inventory"));
        }
        let page: DeltaPage = serde_json::from_str(&body).map_err(storage)?;
        if encoded(&page)?.1 != digest || page.page != index as u64 || page.batch_id != batch {
            return Err(storage("pending page digest or identity differs"));
        }
        result.push(page);
    }
    Ok(result)
}

// Validate the whole prefix against the base without inventing a terminal
// manifest. This refuses duplicate/absent retirements and repeated tombstones
// when the page arrives rather than leaving a poisoned resumable cursor.
fn validate_prefix(base: &Manifest, pages: &[DeltaPage]) -> Result<()> {
    let mut inventory: BTreeMap<_, BTreeSet<_>> = base
        .layers
        .iter()
        .map(|l| {
            (
                l.name.clone(),
                l.chunks.iter().map(|c| c.sha256.clone()).collect(),
            )
        })
        .collect();
    let mut retired = BTreeSet::new();
    let mut changed = BTreeSet::new();
    let mut tombstones = BTreeSet::new();
    let mut count = 0usize;
    for (i, page) in pages.iter().enumerate() {
        page.validate(base, i as u64)?;
        for (layer, chunks) in &page.retired_chunks {
            let held = inventory
                .get_mut(layer)
                .ok_or_else(|| invalid("delta retires unknown layer"))?;
            for chunk in chunks {
                if !retired.insert((layer.clone(), chunk.clone())) || !held.remove(chunk) {
                    return Err(invalid("delta repeats or retires absent chunk"));
                }
                count += 1;
            }
        }
        for (layer, chunks) in &page.changed_chunks {
            let held = inventory
                .get_mut(layer)
                .ok_or_else(|| invalid("delta changes unknown layer"))?;
            for chunk in chunks {
                if !changed.insert((layer.clone(), chunk.sha256.clone()))
                    || !held.insert(chunk.sha256.clone())
                {
                    return Err(invalid("delta repeats or changes held chunk"));
                }
                count += 1;
            }
        }
        for tombstone in &page.tombstones {
            if !inventory.contains_key(&tombstone.layer)
                || !tombstones.insert((tombstone.layer.clone(), tombstone.object_id.clone()))
            {
                return Err(invalid("delta tombstone layer is absent or ID repeats"));
            }
            count += 1;
        }
        if count > policy::MAX_CHUNKS {
            return Err(budget("delta prefix exceeds change budget"));
        }
    }
    if pages.last().is_some_and(|p| p.terminal_manifest.is_some()) {
        policy::assemble_delta(base, pages)?;
    }
    Ok(())
}

impl Store {
    pub(super) fn replay_delta(&self, expected: &Completed) -> Result<Option<Admission>> {
        let _lock = self.lock(false)?;
        replay(&self.connection()?, expected)
    }
    /// Persist one ordered metadata page. This never acquires or owns chunk files.
    pub fn stage_delta_page(&self, page: &DeltaPage) -> Result<DeltaAcquisition> {
        if page.page >= MAX_PAGES as u64 {
            return Err(budget("delta page index exceeds 255"));
        }
        if page.page == MAX_PAGES as u64 - 1 && page.next_page.is_some() {
            return Err(budget("last delta page slot requires a terminal manifest"));
        }
        let (body, digest) = encoded(page)?;
        let page_header = header(page)?;
        let _lock = self.lock(true)?;
        let mut c = self.connection()?;
        let completed: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM delta_receipts WHERE batch=?1)",
                [&page.batch_id],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if completed {
            return Err(stale(
                "delta batch already completed; replay its complete ordered pages",
            ));
        }
        let old = status(&c, &page.batch_id)?;
        if let Some(old) = &old {
            let old_header: String = c
                .query_row(
                    "SELECT header FROM delta_pending WHERE batch=?1",
                    [&page.batch_id],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            if old_header != page_header {
                return Err(stale("pending delta identity differs"));
            }
            let prior: Option<String> = c
                .query_row(
                    "SELECT digest FROM delta_pending_pages WHERE batch=?1 AND page=?2",
                    params![page.batch_id, page.page],
                    |r| r.get(0),
                )
                .optional()
                .map_err(storage)?;
            if let Some(prior) = prior {
                return if prior == digest {
                    Ok(old.clone())
                } else {
                    Err(stale("pending duplicate page differs"))
                };
            }
            if old.next_page != Some(page.page) {
                return Err(stale("delta page is out of order or batch is terminal"));
            }
        } else if page.page != 0 {
            return Err(stale("delta acquisition must start at page zero"));
        }
        if page.context != self.context
            || Self::head_digest(&c)?.as_deref() != Some(page.base_manifest_sha256.as_str())
        {
            return Err(stale(
                "delta acquisition authority or base is not current head",
            ));
        }
        let base = self.manifest(&c, &page.base_manifest_sha256)?;
        let mut pages = load(&c, &page.batch_id)?;
        if pages.len() >= MAX_PAGES {
            return Err(budget("delta exceeds 256 pages"));
        }
        pages.push(page.clone());
        validate_prefix(&base, &pages)?;
        let batch_bytes = old
            .as_ref()
            .map_or(0, |s| s.serialized_bytes)
            .saturating_add(body.len() as u64);
        let (pending_count, pending_bytes): (usize, u64) = c
            .query_row(
                "SELECT count(*),coalesce(sum(bytes),0) FROM delta_pending",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(storage)?;
        if old.is_none() && pending_count >= MAX_PENDING_BATCHES
            || batch_bytes > MAX_BATCH_BYTES as u64
            || pending_bytes.saturating_add(body.len() as u64) > MAX_PENDING_BYTES as u64
        {
            return Err(budget("pending delta metadata budget exceeded"));
        }
        let terminal = page
            .terminal_manifest
            .as_ref()
            .map(|m| m.manifest_sha256.as_str());
        let tx = c.transaction().map_err(storage)?;
        tx.execute("INSERT INTO delta_pending(batch,base,header,target_revision,next_page,page_count,bytes,terminal_sha) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(batch) DO UPDATE SET next_page=excluded.next_page,page_count=excluded.page_count,bytes=excluded.bytes,terminal_sha=excluded.terminal_sha",params![page.batch_id,page.base_manifest_sha256,page_header,serde_json::to_string(&page.target).map_err(storage)?,page.next_page,pages.len(),batch_bytes,terminal]).map_err(storage)?;
        tx.execute(
            "INSERT INTO delta_pending_pages(batch,page,digest,body) VALUES(?1,?2,?3,?4)",
            params![page.batch_id, page.page, digest, body],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        status(&c, &page.batch_id)?.ok_or_else(|| storage("pending delta missing after commit"))
    }
    pub fn pending_delta(&self, batch: &str) -> Result<Option<DeltaAcquisition>> {
        let _lock = self.lock(false)?;
        status(&self.connection()?, batch)
    }
    pub fn pending_delta_pages(&self, batch: &str) -> Result<Vec<DeltaPage>> {
        let _lock = self.lock(false)?;
        load(&self.connection()?, batch)
    }
    pub fn discard_delta(&self, batch: &str) -> Result<bool> {
        let _lock = self.lock(true)?;
        let mut c = self.connection()?;
        let tx = c.transaction().map_err(storage)?;
        let removed = delete_pending(&tx, batch)?;
        tx.commit().map_err(storage)?;
        Ok(removed)
    }
    /// Revalidates caller-owned files. Failure keeps both head and pending cursor.
    /// After completion callers replay with `apply_delta` and the exact pages.
    pub fn finish_delta(
        &self,
        batch: &str,
        inputs: &BTreeMap<String, PathBuf>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Admission> {
        check_cancelled(cancelled)?;
        let pages = self.pending_delta_pages(batch)?;
        if pages.last().is_none_or(|p| p.terminal_manifest.is_none()) {
            return Err(invalid("pending delta is absent or incomplete"));
        }
        self.apply_delta(&pages, inputs, cancelled)
    }
}
