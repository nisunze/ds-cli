//! Persisted local predicates, independent of acquisition and source freshness.
use super::*;
use policy::{AcquisitionReceipt, ReadSource, Revision, SelectionScope};
use serde::Deserialize;

const MAX_MASKS: u64 = 128;
const MAX_NAME_BYTES: usize = 256;
const MAX_MASK_BYTES: u64 = 256 * 1024;
const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024;
const MAX_MASK_NODES: usize = 25_000;
const MAX_MASK_DEPTH: usize = 32;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkArea {
    pub namespace_sha256: String,
    pub name: String,
    pub revision: String,
    pub content_sha256: String,
    pub generation: u64,
    /// None is a deliberately cleared predicate, distinct from no saved mask.
    pub area: Option<Value>,
}
#[derive(Clone, Debug, Serialize)]
pub struct WorkAreaUpdate {
    pub mask: WorkArea,
    pub changed: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewPredicate {
    Intersects,
    AllRetainedRows,
}

/// Owns the immutable mask and source pin captured together for this view.
pub struct LocalView {
    mask: WorkArea,
    pin: Pin,
}

#[derive(Clone, Debug, Serialize)]
pub struct LocalReadReceipt {
    pub work_area: WorkArea,
    pub predicate: ViewPredicate,
    pub manifest_sha256: String,
    pub context: Context,
    pub source_kind: SourceKind,
    pub read_source: ReadSource,
    pub source_revision: Revision,
    pub selection: SelectionScope,
    pub acquisition_filter: Option<String>,
    /// Unmodified source evidence; masking establishes no new coverage claim.
    pub acquisition: AcquisitionReceipt,
    pub layer: String,
    /// Observed matches in this retained source only; no uncovered-area claim.
    pub rows: u64,
    pub elapsed_ms: u64,
}

pub(super) fn schema(c: &Connection, namespace: &str) -> Result<()> {
    let (masks,clock):(bool,bool)=c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='local_work_areas'),EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='local_work_area_clock')",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(storage)?;
    if masks != clock {
        return Err(storage("work-area catalogue/clock schema is corrupt"));
    }
    if !clock {
        let tx = c.unchecked_transaction().map_err(storage)?;
        tx.execute_batch("CREATE TABLE local_work_areas(name TEXT PRIMARY KEY,revision TEXT NOT NULL,content_sha TEXT NOT NULL,generation INTEGER NOT NULL,body TEXT NOT NULL,bytes INTEGER NOT NULL); CREATE TABLE local_work_area_clock(id INTEGER PRIMARY KEY CHECK(id=1),generation INTEGER NOT NULL,sha TEXT NOT NULL);").map_err(storage)?;
        tx.execute(
            "INSERT INTO local_work_area_clock VALUES(1,0,?1)",
            [clock_digest(namespace, 0)?],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
    }
    let (generation, sha): (u64, String) = c
        .query_row(
            "SELECT generation,sha FROM local_work_area_clock WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(storage)?;
    let maximum: u64 = c
        .query_row(
            "SELECT coalesce(max(generation),0) FROM local_work_areas",
            [],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if clock_digest(namespace, generation)? != sha || generation < maximum {
        return Err(storage("work-area generation clock is corrupt"));
    }
    Ok(())
}

pub(super) fn clock_digest(namespace: &str, generation: u64) -> Result<String> {
    canonical_hash(
        b"ds.snapshot-work-area-clock/v1\0",
        &(namespace, generation),
    )
}

fn invalid(reason: &str) -> Refusal {
    refusal(RefusalCode::SnapshotInvalid, reason)
}
fn exceeded(reason: &str) -> Refusal {
    refusal(RefusalCode::SnapshotBudgetExceeded, reason)
}
fn name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > MAX_NAME_BYTES
        || name.trim() != name
        || name.chars().any(char::is_control)
    {
        return Err(invalid("invalid local work-area name"));
    }
    Ok(())
}
fn expected(revision: Option<&str>) -> Result<()> {
    if revision.is_some_and(|v| !policy::sha256(v)) {
        return Err(invalid("invalid expected work-area revision"));
    }
    Ok(())
}
struct Counter(u64);
impl Write for Counter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(b.len() as u64);
        if self.0 > MAX_MASK_BYTES {
            return Err(std::io::Error::other("work-area mask exceeds 256 KiB"));
        }
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
// Walk without recursion or broad clones before invoking the existing geometry
// admission/predicate owner. Bound the traversal stack while children are added.
fn shape_budget(area: &Value) -> Result<()> {
    let mut stack = vec![(area, 0usize)];
    let mut visited = 0usize;
    while let Some((value, depth)) = stack.pop() {
        visited += 1;
        if depth > MAX_MASK_DEPTH || visited > MAX_MASK_NODES {
            return Err(exceeded("work-area JSON exceeds node/depth budget"));
        }
        let children: Box<dyn Iterator<Item = &Value> + '_> = match value {
            Value::Array(v) => Box::new(v.iter()),
            Value::Object(v) => Box::new(v.values()),
            _ => continue,
        };
        for child in children {
            if stack.len() + visited >= MAX_MASK_NODES {
                return Err(exceeded("work-area JSON exceeds node budget"));
            }
            stack.push((child, depth + 1));
        }
    }
    Ok(())
}
fn encoded_size(key: &str, area: Option<&Value>) -> Result<u64> {
    if let Some(area) = area {
        shape_budget(area)?;
    }
    let mut count = Counter(0);
    serde_json::to_writer(&mut count, &(key, area))
        .map_err(|_| exceeded("work-area mask exceeds 256 KiB"))?;
    Ok(count.0)
}
fn encoded(key: &str, area: Option<&Value>) -> Result<String> {
    if let Some(area) = area {
        ds_command_kernel::reference_cache::validate_area(area).map_err(|e| invalid(&e))?;
    }
    let mut value = serde_json::to_value((key, area)).map_err(storage)?;
    value.sort_all_objects();
    serde_json::to_string(&value).map_err(storage)
}
fn content_digest(body: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"ds.snapshot-work-area-content/v1\0");
    digest.update(body.as_bytes());
    format!("{:x}", digest.finalize())
}
fn revision(namespace: &str, content: &str, generation: u64) -> Result<String> {
    canonical_hash(
        b"ds.snapshot-work-area-revision/v1\0",
        &(namespace, content, generation),
    )
}
fn next_generation(c: &Connection) -> Result<u64> {
    let generation: u64 = c
        .query_row(
            "SELECT generation FROM local_work_area_clock WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if generation >= i64::MAX as u64 {
        return Err(exceeded("work-area revision clock is exhausted"));
    }
    Ok(generation + 1)
}
fn get(c: &Connection, namespace: &str, key: &str) -> Result<Option<WorkArea>> {
    type StoredMask = (String, String, u64, String, u64);
    let saved: Option<StoredMask> = c
        .query_row(
            "SELECT revision,content_sha,generation,body,bytes FROM local_work_areas WHERE name=?1",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .map_err(storage)?;
    saved
        .map(
            |(saved_revision, content_sha256, generation, body, bytes)| {
                if bytes > MAX_MASK_BYTES || body.len() as u64 != bytes {
                    return Err(storage("work-area stored byte count differs"));
                }
                let (saved_name, area): (String, Option<Value>) =
                    serde_json::from_str(&body).map_err(storage)?;
                name(&saved_name)?;
                if saved_name != key
                    || content_digest(&body) != content_sha256
                    || revision(namespace, &content_sha256, generation)? != saved_revision
                {
                    return Err(storage("work-area stored identity differs"));
                }
                if encoded_size(key, area.as_ref())? != bytes
                    || encoded(key, area.as_ref())? != body
                {
                    return Err(storage("work-area stored body is not canonical"));
                }
                Ok(WorkArea {
                    namespace_sha256: namespace.into(),
                    name: saved_name,
                    revision: saved_revision,
                    content_sha256,
                    generation,
                    area,
                })
            },
        )
        .transpose()
}

impl Store {
    pub fn namespace_sha256(&self) -> Result<String> {
        canonical_hash(b"ds.snapshot-room/v1\0", &(&self.context, self.source))
    }
    pub fn work_area(&self, key: &str) -> Result<Option<WorkArea>> {
        name(key)?;
        let namespace = self.namespace_sha256()?;
        let _lock = self.lock(false)?;
        get(&self.connection()?, &namespace, key)
    }
    /// Generation-fenced CAS. Expected None creates only an absent name;
    /// an exact duplicate is a no-op only after expected revision matches.
    pub fn set_work_area(
        &self,
        key: &str,
        area: Option<&Value>,
        expected_revision: Option<&str>,
    ) -> Result<WorkAreaUpdate> {
        name(key)?;
        expected(expected_revision)?;
        // Count before geometry validation, owned copies or hashing.
        let bytes = encoded_size(key, area)?;
        let namespace = self.namespace_sha256()?;
        let _lock = self.lock(true)?;
        let mut c = self.connection()?;
        let current = get(&c, &namespace, key)?;
        let old_bytes: Option<u64> = c
            .query_row(
                "SELECT bytes FROM local_work_areas WHERE name=?1",
                [key],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        let (count, total): (u64, u64) = c
            .query_row(
                "SELECT count(*),coalesce(sum(bytes),0) FROM local_work_areas",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(storage)?;
        if current.is_none() && count >= MAX_MASKS
            || total
                .saturating_sub(old_bytes.unwrap_or(0))
                .saturating_add(bytes)
                > MAX_TOTAL_BYTES
        {
            return Err(exceeded(
                "local work-area catalogue exceeds count/byte budget",
            ));
        }
        if current.as_ref().map(|m| m.revision.as_str()) != expected_revision {
            return Err(refusal(
                RefusalCode::SnapshotStaleView,
                "work-area generation CAS differs",
            ));
        }
        let body = encoded(key, area)?;
        let content_sha256 = content_digest(&body);
        if let Some(current) = &current {
            if current.content_sha256 == content_sha256 {
                return Ok(WorkAreaUpdate {
                    mask: current.clone(),
                    changed: false,
                });
            }
        }
        let generation = next_generation(&c)?;
        let revision = revision(&namespace, &content_sha256, generation)?;
        let mask = WorkArea {
            namespace_sha256: namespace,
            name: key.into(),
            revision,
            content_sha256,
            generation,
            area: area.cloned(),
        };
        let tx = c.transaction().map_err(storage)?;
        tx.execute("INSERT INTO local_work_areas(name,revision,content_sha,generation,body,bytes) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(name) DO UPDATE SET revision=excluded.revision,content_sha=excluded.content_sha,generation=excluded.generation,body=excluded.body,bytes=excluded.bytes",params![key,mask.revision,mask.content_sha256,mask.generation,body,bytes]).map_err(storage)?;
        tx.execute(
            "UPDATE local_work_area_clock SET generation=?1,sha=?2 WHERE id=1",
            params![
                generation,
                clock_digest(&self.namespace_sha256()?, generation)?
            ],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(WorkAreaUpdate {
            mask,
            changed: true,
        })
    }
    /// Removes only this saved local predicate. It never removes source rows.
    pub fn remove_work_area(&self, key: &str, expected_revision: &str) -> Result<bool> {
        name(key)?;
        expected(Some(expected_revision))?;
        let namespace = self.namespace_sha256()?;
        let _lock = self.lock(true)?;
        let mut c = self.connection()?;
        let Some(current) = get(&c, &namespace, key)? else {
            return Ok(false);
        };
        if current.revision != expected_revision {
            return Err(refusal(
                RefusalCode::SnapshotStaleView,
                "work-area removal CAS differs",
            ));
        }
        let generation = next_generation(&c)?;
        let tx = c.transaction().map_err(storage)?;
        tx.execute("DELETE FROM local_work_areas WHERE name=?1", [key])
            .map_err(storage)?;
        tx.execute(
            "UPDATE local_work_area_clock SET generation=?1,sha=?2 WHERE id=1",
            params![
                generation,
                clock_digest(&self.namespace_sha256()?, generation)?
            ],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(true)
    }
    /// Capture a saved mask against an already immutable source pin. No head
    /// lookup, acquisition or source refresh occurs at this boundary.
    pub fn capture_view(&self, key: &str, pin: Pin) -> Result<LocalView> {
        name(key)?;
        if pin.root != self.root
            || pin.manifest.context != self.context
            || pin.manifest.source_kind != self.source
        {
            return Err(refusal(
                RefusalCode::SnapshotContextMismatch,
                "local view/pin namespace differs",
            ));
        }
        let namespace = self.namespace_sha256()?;
        let _lock = self.lock(false)?;
        let mask = get(&self.connection()?, &namespace, key)?.ok_or_else(|| {
            invalid("local work-area mask is absent; explicitly create or clear it")
        })?;
        Ok(LocalView { mask, pin })
    }
}
impl LocalView {
    pub fn work_area(&self) -> &WorkArea {
        &self.mask
    }
    pub fn manifest(&self) -> &Manifest {
        self.pin.manifest()
    }
    pub fn visit_layer(
        &self,
        layer: &str,
        cancelled: &dyn Fn() -> bool,
        visitor: &mut dyn FnMut(Value) -> std::result::Result<(), String>,
    ) -> Result<LocalReadReceipt> {
        let started = Instant::now();
        let rows = self
            .pin
            .visit_layer(layer, self.mask.area.as_ref(), cancelled, visitor)?;
        let m = self.pin.manifest();
        Ok(LocalReadReceipt {
            work_area: self.mask.clone(),
            predicate: if self.mask.area.is_some() {
                ViewPredicate::Intersects
            } else {
                ViewPredicate::AllRetainedRows
            },
            manifest_sha256: m.manifest_sha256.clone(),
            context: m.context.clone(),
            source_kind: m.source_kind,
            read_source: m.read_source,
            source_revision: m.revision.clone(),
            selection: m.selection,
            acquisition_filter: m.acquisition_filter.clone(),
            acquisition: m.acquisition.clone(),
            layer: layer.into(),
            rows,
            elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        })
    }
}
