//! Prepared local layers for a native host: the catalogue file, the payload
//! directory, and nothing else.
//!
//! A prepared local layer is a bounded catalogue row — a name, an origin, a
//! geometry type, a flat style, a column schema, a feature count, and wherever
//! its payload is parked. The browser keeps its catalogue in IndexedDB; `ds`
//! and the desktop shell keep theirs in a file here, one per lane and DS
//! account, under the same root as the overlay registry and the visibility
//! store.
//!
//! **Every decision is the kernel's.** `ds_command_kernel::local_layers` mints
//! the id, fills every default, picks the colour, normalises the origin,
//! fences the scope and names what a removal may delete. This module performs
//! the I/O on that answer and decides nothing else: it never invents an id,
//! never repairs a row, never widens a bound and never deletes a file the
//! receipt did not name. That is why a removal here provably cannot touch an
//! original source file — the kernel never names one, so there is nothing to
//! act on.
//!
//! What lives on disk, for a scope of lane `run` and account `u1`:
//!
//! ```text
//! <root>/prepared/run/u1/catalogue.json      the registry, exactly as the kernel answers it
//! <root>/prepared/run/u1/catalogue.lock      the exclusive lock every writer takes
//! <root>/prepared/run/u1/payloads/<id>.geojson   one layer's copied features
//! ```
//!
//! The lock discipline is the overlay registry's (`lib.rs`): one exclusive
//! lock per store, a bounded read, atomic replacement, an absent catalogue
//! reads as empty and creates nothing, and a malformed catalogue is refused
//! rather than repaired. The lock is taken only once the kernel has ADMITTED
//! a change — the catalogue is read first, the kernel answers over it in
//! memory, and a refusal returns before this host has created a directory, a
//! lock or a file. So an op the kernel refuses leaves a store that was never
//! opened with nothing in it at all, and the answer that is persisted is
//! always the second one, taken again over the catalogue read under the lock.
//!
//! Repair is `restore`'s job, and `restore` belongs to a host whose rows are
//! loose persisted records; this store's file is a typed registry the kernel
//! itself wrote, so a file that no longer parses is a fault to look at, never
//! one to overwrite.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::private;
use fs2::FileExt;
use serde_json::{json, Value};

use ds_command_kernel::geometry_admission;
use ds_command_kernel::local_layers::{self as kernel, REGISTRY_SCHEMA};
pub use ds_command_kernel::local_layers::{GeometryType, Host, LocalLayer, Registry, Scope};

/// The catalogue is read *and written* under one bound, so a store can never
/// grow past what it can reopen. The overlay registry's bound, for the same
/// reason: a local catalogue that needs a megabyte of JSON is a fault.
pub const MAX_CATALOGUE_BYTES: usize = 1024 * 1024;

/// One copied payload. Large enough for a field survey's own GeoJSON, small
/// enough that a mistyped path cannot fill a laptop.
pub const MAX_PAYLOAD_BYTES: u64 = 64 * 1024 * 1024;

/// Where these rows live, said the same way the overlay registry says it.
pub const PERSISTED: &str = "native_local";

/// Why an op did nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// The kernel refused. The code is the kernel's own, closed vocabulary
    /// (`malformed_descriptor`, `duplicate_layer`, `unknown_layer`,
    /// `scope_mismatch`, `unsupported_source_kind`)
    /// and callers re-raise it literally rather than renaming it.
    Refused { code: String, message: String },
    /// The file named for a payload is not an admissible bounded GeoJSON
    /// feature collection of the declared geometry. Nothing was read into the
    /// store and nothing on disk changed.
    Payload(String),
    /// The store could not read or persist, or was asked something it could
    /// not build a request from.
    Store(String),
}

impl StoreError {
    pub fn code(&self) -> &str {
        match self {
            Self::Refused { code, .. } => code,
            Self::Payload(_) => "invalid_payload",
            Self::Store(_) => "local_layer_refused",
        }
    }
    pub fn message(&self) -> &str {
        match self {
            Self::Refused { message, .. } | Self::Payload(message) | Self::Store(message) => {
                message
            }
        }
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for StoreError {}

/// What a native host asks of its own catalogue. Four questions, because four
/// is what a machine-local catalogue has: what is in it, admit this, rename
/// that, remove that. Everything else the kernel can answer belongs to a host
/// with loose persisted rows to reopen.
#[derive(Clone, Debug)]
pub enum Op {
    /// The bounded rows, and how many the catalogue holds.
    List { limit: Option<usize> },
    /// Admit one new layer, copying its payload if one was named.
    Register(Register),
    /// Rename one layer.
    Rename { id: String, name: String },
    /// Remove one layer, then delete exactly what the receipt released.
    Remove { id: String },
}

/// What the host knows about a layer it is asking for. Everything else — the
/// id, the colour, the style, the source label, the origin — is the kernel's
/// answer.
#[derive(Clone, Debug)]
pub struct Register {
    pub name: String,
    pub geometry: GeometryType,
    /// The container this layer is filed under. Absent takes the kernel's
    /// default for the kind, or the payload file's own name.
    pub source_name: Option<String>,
    /// A GeoJSON feature collection on this host to copy into the payload
    /// directory. The original is never moved, modified or deleted.
    pub from: Option<PathBuf>,
    /// The host's clock. Absent takes this machine's, which is what a
    /// command-line host has; a test passes one so the minted id is fixed.
    pub now_ms: Option<i64>,
}

/// Read one geometry word the way the descriptor spells it.
pub fn geometry_type(word: &str) -> Option<GeometryType> {
    match word.trim().to_ascii_lowercase().as_str() {
        "point" => Some(GeometryType::Point),
        "linestring" | "line" => Some(GeometryType::LineString),
        "polygon" => Some(GeometryType::Polygon),
        _ => None,
    }
}

/// Ask this machine's default store.
pub fn execute(scope: &Scope, op: Op) -> Result<Value, StoreError> {
    let root = super::default_root().map_err(StoreError::Store)?;
    execute_at(&root, scope, op)
}

/// Ask the store under `root` for the scope's catalogue.
///
/// The answer is the kernel's, with what this host actually did to its own
/// files beside it: `receipt`, `changed`, `revision`, `persisted`, `scope`,
/// and — only where one happened — the `payload` it copied in or the files it
/// `released`.
///
/// Nothing here is created in order to ask a question. The catalogue is read
/// if it is there, the kernel answers over that registry in memory, and only
/// an answer that changed something opens this store for writing. A refusal
/// therefore leaves a store that was never opened exactly as it was — no lane
/// directory, no lock file, nothing — because a question this host can answer
/// from what it read is never one it has to take a lock to ask.
pub fn execute_at(root: &Path, scope: &Scope, op: Op) -> Result<Value, StoreError> {
    let dir = scope_dir(root, scope)?;
    let catalogue = dir.join("catalogue.json");

    // The clock is read once, before anything is asked, so the provisional
    // answer and the authoritative one below are the same question about the
    // same instant.
    let op = match op {
        Op::Register(register) => Op::Register(Register {
            now_ms: Some(register.now_ms.unwrap_or_else(now_ms)),
            ..register
        }),
        other => other,
    };

    // A named payload is read, bounded and admitted BEFORE this store is
    // touched at all: a file that is not an admissible source refuses without
    // ever making another writer wait, and without a half-written store to
    // undo. Reading the caller's own file is not a write to this store.
    let staged = match &op {
        Op::Register(register) => stage(register.from.as_deref(), register.geometry)?,
        _ => None,
    };

    // The unlocked read, and the provisional answer over it. An absent
    // catalogue reads as an empty registry and needs no lock — there is
    // nothing yet for another writer to race over — and a catalogue that is
    // there is only ever replaced atomically, so an unlocked reader sees one
    // whole registry or the other and never half of either. A read, and any
    // answer that changed nothing, ends here with nothing created.
    let registry = load(&catalogue, scope)?;
    let answer = ask(request(&op, &registry, scope, staged.as_ref())?)?;
    if answer["changed"] != true {
        return Ok(reply(&answer, scope, Value::Null, vec![]));
    }

    // The kernel admitted a change, so now — and only now — this host opens
    // its own store: the lane directory, the lock every writer shares, and a
    // second reading of the catalogue under it. Another writer may have been
    // admitted between the two readings, so it is the answer over the LOCKED
    // catalogue that is authoritative: the registry this write replaces, the
    // sequence and palette cursor it mints from and the receipt this host
    // acts on all come from that second ask.
    private::create_dir_all(&dir).map_err(io_error)?;
    let lock = private::open_options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("catalogue.lock"))
        .map_err(io_error)?;
    lock.lock_exclusive().map_err(io_error)?;

    let registry = load(&catalogue, scope)?;
    let answer = ask(request(&op, &registry, scope, staged.as_ref())?)?;
    let next: Registry = serde_json::from_value(answer["registry"].clone()).map_err(|error| {
        StoreError::Store(format!("the kernel's registry is unreadable: {error}"))
    })?;
    let receipt = &answer["receipt"];

    // Every file this answer touches is named BEFORE anything is written: the
    // payload under the id the kernel just minted, and the files the receipt
    // released. An id that is not one path segment names nothing here, and
    // says so now, while the catalogue is still exactly what it was — a
    // refusal never follows a write.
    let payload_file = match &staged {
        Some(_) => Some(payload_path(
            &dir,
            receipt["id"]
                .as_str()
                .ok_or_else(|| StoreError::Store("the admitted layer has no id".into()))?,
        )?),
        None => None,
    };
    let releases = release_plan(&dir, receipt)?;

    // The payload is written before the catalogue, so a catalogue row never
    // names a file that is not there.
    let mut payload = Value::Null;
    if let (Some(staged), Some(file)) = (&staged, &payload_file) {
        private::create_dir_all(&payloads_dir(&dir)).map_err(io_error)?;
        write_atomic(file, payloads_dir(&dir).as_path(), &staged.bytes)?;
        payload = json!({
            "path": file.to_string_lossy(),
            "features": staged.features,
            "bytes": staged.bytes.len(),
            "source": staged.source_path,
        });
    }

    if answer["changed"] == true {
        if let Err(error) = persist(&catalogue, &dir, &next) {
            // Nothing was admitted, so nothing may be left behind either.
            if let Some(file) = &payload_file {
                let _ = fs::remove_file(file);
            }
            return Err(error);
        }
    }

    // Delete exactly what the receipt released, and only after the row that
    // owned it is gone: a crash between the two leaves an unreferenced file,
    // never a row pointing at a file that is not there.
    let released = release(&releases)?;

    Ok(reply(&answer, scope, payload, released))
}

/// The kernel's answer, plus what this host did with it.
fn reply(answer: &Value, scope: &Scope, payload: Value, released: Vec<String>) -> Value {
    let mut reply = json!({
        "receipt": answer["receipt"].clone(),
        "changed": answer["changed"].clone(),
        "revision": answer["registry"]["revision"].clone(),
        "scope": serde_json::to_value(scope).unwrap_or(Value::Null),
        "persisted": PERSISTED,
    });
    if !payload.is_null() {
        reply["payload"] = payload;
    }
    if !released.is_empty() {
        reply["released"] = json!(released);
    }
    reply
}

// ── the kernel ──────────────────────────────────────────────────────────

/// One question, asked the only way this store asks one.
fn ask(op: Value) -> Result<Value, StoreError> {
    ask_as(kernel::SCHEMA, op)
}

/// The seam itself. Only `ask` calls it in the product — the schema this store
/// speaks is not a choice — and the recorded corpus drives it with the one
/// case that names a schema of its own.
fn ask_as(schema: &str, op: Value) -> Result<Value, StoreError> {
    let request = serde_json::to_vec(&json!({"schema": schema, "op": op}))
        .map_err(|error| StoreError::Store(error.to_string()))?;
    let answer = kernel::evaluate(&request).map_err(StoreError::Store)?;
    let answer: Value = serde_json::from_str(&answer).map_err(|error| {
        StoreError::Store(format!("the kernel's answer is unreadable: {error}"))
    })?;
    if answer["outcome"] == "refused" {
        return Err(StoreError::Refused {
            code: answer["refusal"]["code"]
                .as_str()
                .unwrap_or(kernel::MALFORMED_DESCRIPTOR)
                .to_owned(),
            message: answer["refusal"]["message"]
                .as_str()
                .unwrap_or("the local layer was refused")
                .to_owned(),
        });
    }
    Ok(answer)
}

/// The op, in the wire the kernel reads. Every field this host supplies is a
/// clock, a counter or something the caller named — never a decision.
fn request(
    op: &Op,
    registry: &Registry,
    scope: &Scope,
    staged: Option<&Payload>,
) -> Result<Value, StoreError> {
    let registry = serde_json::to_value(registry)
        .map_err(|error| StoreError::Store(format!("the catalogue is unreadable: {error}")))?;
    let scope_value = serde_json::to_value(scope)
        .map_err(|error| StoreError::Store(format!("the scope is unreadable: {error}")))?;
    Ok(match op {
        Op::List { limit } => {
            let mut op = json!({"kind": "list", "registry": registry, "scope": scope_value});
            if let Some(limit) = limit {
                op["limit"] = json!(limit);
            }
            op
        }
        Op::Register(register) => {
            let now_ms = register.now_ms.unwrap_or_else(now_ms);
            json!({
                "kind": "register",
                "registry": registry,
                "scope": scope_value,
                "now_ms": now_ms,
                "sequence": next_sequence(&registry, now_ms),
                "colorIndex": palette_cursor(&registry),
                "draft": draft(register, staged),
            })
        }
        Op::Rename { id, name } => json!({
            "kind": "update",
            "registry": registry,
            "scope": scope_value,
            "id": id,
            "change": {"kind": "rename", "name": name},
        }),
        Op::Remove { id } => json!({
            "kind": "remove",
            "registry": registry,
            "scope": scope_value,
            "id": id,
        }),
    })
}

/// What the host knows, in the descriptor's own spelling. A copied file is an
/// imported layer and says where it came from; a layer with no payload is a
/// manual one with no features yet.
fn draft(register: &Register, staged: Option<&Payload>) -> Value {
    let mut draft = json!({"name": register.name, "geometryType": register.geometry});
    match staged {
        Some(payload) => {
            draft["sourceKind"] = json!("import");
            draft["featureCount"] = json!(payload.features);
            draft["sourceName"] = json!(match &register.source_name {
                Some(named) => named.clone(),
                None => payload.file_name.clone(),
            });
            let mut metadata = json!({
                "kind": "import_file",
                "fileName": payload.file_name,
                "sourceName": payload.file_name,
                "sourcePath": payload.source_path,
                "sizeBytes": payload.bytes.len(),
            });
            if let Some(modified) = payload.last_modified {
                metadata["lastModified"] = json!(modified);
            }
            draft["sourceMetadata"] = metadata;
        }
        None => {
            if let Some(named) = &register.source_name {
                draft["sourceName"] = json!(named);
            }
        }
    }
    draft
}

/// The sequence half of the id, taken under the lock from the catalogue
/// itself: the lowest one this clock reading has not already used. Two writers
/// racing on the same millisecond therefore mint different ids, because each
/// reads the catalogue the other has already written.
fn next_sequence(registry: &Value, now_ms: i64) -> u64 {
    let taken = |id: &str| {
        registry["layers"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|layer| layer["id"].as_str() == Some(id))
    };
    let mut sequence = 0u64;
    while taken(&format!("sketch-{now_ms}-{sequence}")) {
        sequence += 1;
    }
    sequence
}

/// The palette cursor a fresh store has reached: one colour per layer already
/// in it, in turn, exactly as a browser profile hands them out.
fn palette_cursor(registry: &Value) -> u64 {
    registry["layers"].as_array().map_or(0, Vec::len) as u64
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or(1)
        .max(1)
}

// ── the files ───────────────────────────────────────────────────────────

fn io_error(_: impl std::fmt::Display) -> StoreError {
    StoreError::Store("the prepared local layer store could not be read or persisted".into())
}

fn opened(scope: &Scope) -> Registry {
    Registry {
        schema: REGISTRY_SCHEMA,
        revision: 0,
        scope: scope.clone(),
        layers: vec![],
    }
}

/// One path segment, with no way out of the directory it names. The kernel
/// bounds a lane, a uid and an id as text; only this host turns one into a
/// path, so only this host has to prove it stays inside its own store.
fn safe_segment(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= 200
        && !value
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\' || c == ':')
        && Path::new(value).components().count() == 1
        && matches!(
            Path::new(value).components().next(),
            Some(Component::Normal(_))
        )
}

/// `<root>/prepared/<lane>/<uid>`. A browser profile has no directory here:
/// its catalogue is IndexedDB, and this store never speaks for it.
pub fn scope_dir(root: &Path, scope: &Scope) -> Result<PathBuf, StoreError> {
    if scope.host != Host::Native {
        return Err(StoreError::Store(
            "a prepared local layer store on disk is a native scope; a browser profile keeps its own catalogue".into(),
        ));
    }
    let (Some(lane), Some(uid)) = (scope.lane.as_deref(), scope.uid.as_deref()) else {
        return Err(StoreError::Store(
            "a native prepared local layer scope needs a lane and an account uid".into(),
        ));
    };
    if !safe_segment(lane) || !safe_segment(uid) {
        return Err(StoreError::Store(
            "a prepared local layer lane and account uid must each be one bounded path segment"
                .into(),
        ));
    }
    Ok(root.join("prepared").join(lane).join(uid))
}

fn payloads_dir(dir: &Path) -> PathBuf {
    dir.join("payloads")
}

/// The one file a layer owns. The id is held to a single safe segment here as
/// well: a catalogue is a file an operator can edit, and an id that walked out
/// of the payload directory would be the one way this store could touch
/// something that is not its own.
fn payload_path(dir: &Path, id: &str) -> Result<PathBuf, StoreError> {
    if !safe_segment(id) {
        return Err(StoreError::Store(format!(
            "local layer id {id} is not one bounded path segment, so it names no payload here"
        )));
    }
    Ok(payloads_dir(dir).join(format!("{id}.geojson")))
}

fn load(path: &Path, scope: &Scope) -> Result<Registry, StoreError> {
    match File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_CATALOGUE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(io_error)?;
            if bytes.len() > MAX_CATALOGUE_BYTES {
                return Err(StoreError::Store(
                    "the prepared local layer catalogue exceeds 1 MiB".into(),
                ));
            }
            serde_json::from_slice::<Registry>(&bytes).map_err(|_| {
                StoreError::Store(
                    "the prepared local layer catalogue is malformed; repair it before editing"
                        .into(),
                )
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(opened(scope)),
        Err(error) => Err(io_error(error)),
    }
}

fn persist(path: &Path, dir: &Path, registry: &Registry) -> Result<(), StoreError> {
    let bytes = serde_json::to_vec(registry)
        .map_err(|error| StoreError::Store(format!("the catalogue is unwritable: {error}")))?;
    // Bound the write, not only the read: a store that could be written past
    // what it can be reopened at would brick itself one layer at a time.
    if bytes.len() > MAX_CATALOGUE_BYTES {
        return Err(StoreError::Store(
            "this catalogue would exceed 1 MiB; remove a layer before adding another".into(),
        ));
    }
    write_atomic(path, dir, &bytes)
}

fn write_atomic(path: &Path, dir: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let mut staged = tempfile::NamedTempFile::new_in(dir).map_err(io_error)?;
    staged.write_all(bytes).map_err(io_error)?;
    staged.flush().map_err(io_error)?;
    staged.as_file().sync_all().map_err(io_error)?;
    staged.persist(path).map_err(io_error)?;
    #[cfg(unix)]
    File::open(dir)
        .and_then(|handle| handle.sync_all())
        .map_err(io_error)?;
    Ok(())
}

/// The row is gone but its payload could not be: say so rather than report a
/// clean removal over a file that is still there.
fn orphan(file: &Path) -> StoreError {
    StoreError::Store(format!(
        "the layer left the catalogue but its payload {} could not be deleted",
        file.to_string_lossy()
    ))
}

/// The files the receipt named for release, resolved and nothing else.
///
/// `features_payload` is the one record this host keeps, so it is the one it
/// can delete. `upload_chunks` names a browser's parked chunks; a native store
/// has none, so it is left alone. Neither is ever a source file: the kernel
/// does not name one, and this function can only resolve what it was handed —
/// and only to a path inside this store's own payload directory, or not at all.
fn release_plan(dir: &Path, receipt: &Value) -> Result<Vec<PathBuf>, StoreError> {
    let mut files = Vec::new();
    for item in receipt["release"].as_array().into_iter().flatten() {
        if item["kind"] != "features_payload" {
            continue;
        }
        let Some(id) = item["id"].as_str() else {
            continue;
        };
        files.push(payload_path(dir, id)?);
    }
    Ok(files)
}

/// Delete exactly the files the plan named. A file that is already gone is
/// not a fault: the row owned nothing on disk.
fn release(files: &[PathBuf]) -> Result<Vec<String>, StoreError> {
    let mut released = Vec::new();
    for file in files {
        match fs::remove_file(file) {
            Ok(()) => released.push(file.to_string_lossy().into_owned()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(orphan(file)),
        }
    }
    Ok(released)
}

// ── the payload ─────────────────────────────────────────────────────────

/// A GeoJSON feature collection this host read, admitted and will copy.
struct Payload {
    bytes: Vec<u8>,
    features: u64,
    file_name: String,
    source_path: String,
    last_modified: Option<i64>,
}

/// Read the named file, bound it, and admit it as a feature collection of the
/// declared geometry. The bytes are kept exactly as they were read: what lands
/// in the payload directory is a copy, not a re-rendering, and the original is
/// only ever opened for reading.
fn stage(from: Option<&Path>, geometry: GeometryType) -> Result<Option<Payload>, StoreError> {
    let Some(from) = from else {
        return Ok(None);
    };
    let refuse = |why: String| StoreError::Payload(why);
    let shown = from.to_string_lossy().into_owned();

    let metadata = fs::metadata(from).map_err(|_| {
        refuse(format!(
            "{shown} does not name a readable file on this host"
        ))
    })?;
    if !metadata.is_file() {
        return Err(refuse(format!(
            "{shown} is not a regular file; one layer is prepared from one file"
        )));
    }
    let file = File::open(from).map_err(|_| {
        refuse(format!(
            "{shown} does not name a readable file on this host"
        ))
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_PAYLOAD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| refuse(format!("{shown} could not be read")))?;
    if bytes.len() as u64 > MAX_PAYLOAD_BYTES {
        return Err(refuse(format!(
            "{shown} exceeds the {} MiB a prepared local layer payload may hold",
            MAX_PAYLOAD_BYTES / (1024 * 1024)
        )));
    }

    let features = admit_feature_collection(&bytes, geometry, &shown)?;

    let file_name = from
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| refuse(format!("{shown} has no file name")))?;
    // Lexical only: resolving links is not this store's business, and a
    // recorded origin is provenance, never a capability — a removal's receipt
    // never names it.
    let source_path = std::path::absolute(from)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or(shown);
    let last_modified = metadata
        .modified()
        .ok()
        .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_millis() as i64)
        .filter(|at| *at > 0);

    Ok(Some(Payload {
        bytes,
        features,
        file_name,
        source_path,
        last_modified,
    }))
}

/// A bounded GeoJSON `FeatureCollection` whose every feature carries the
/// declared geometry, or its `Multi` form — one layer paints one geometry, and
/// a collection that mixes them would render as something the descriptor does
/// not describe.
fn admit_feature_collection(
    bytes: &[u8],
    geometry: GeometryType,
    shown: &str,
) -> Result<u64, StoreError> {
    let refuse = |why: &str| StoreError::Payload(format!("{shown} {why}"));
    let document: Value = serde_json::from_slice(bytes).map_err(|_| refuse("is not JSON"))?;
    if document["type"] != "FeatureCollection" {
        return Err(refuse("is not a GeoJSON FeatureCollection"));
    }
    let features = document["features"]
        .as_array()
        .ok_or_else(|| refuse("has no features array"))?;
    let (declared, multi) = match geometry {
        GeometryType::Point => ("Point", "MultiPoint"),
        GeometryType::LineString => ("LineString", "MultiLineString"),
        GeometryType::Polygon => ("Polygon", "MultiPolygon"),
    };
    for (index, feature) in features.iter().enumerate() {
        if feature["type"] != "Feature" {
            return Err(refuse(&format!("holds a non-Feature at {index}")));
        }
        let kind = feature["geometry"]["type"].as_str().unwrap_or_default();
        if kind != declared && kind != multi {
            return Err(refuse(&format!(
                "holds {} geometry at {index}; this layer is declared {declared}",
                if kind.is_empty() { "no" } else { kind }
            )));
        }
        if !feature["geometry"]["coordinates"].is_array() {
            return Err(refuse(&format!("has no coordinates at {index}")));
        }
    }
    // Every geometry is one DS admits. The bytes are copied as read, so each
    // is judged as the other file doors judge it: its altitude dropped first.
    let judged: Vec<Value> = features
        .iter()
        .map(|feature| {
            let mut geometry = feature["geometry"].clone();
            geometry_admission::drop_altitude(&mut geometry);
            json!({"id": feature["id"], "geometry": geometry})
        })
        .collect();
    geometry_admission::admit_features(&judged).map_err(|refusals| {
        StoreError::Payload(format!(
            "{shown} {}: {refusals}; nothing was prepared",
            refusals.code()
        ))
    })?;
    Ok(features.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;
    use std::collections::BTreeSet;

    /// The kernel's own recorded corpus, byte-identical to the copy ds-web's
    /// browser replays. The store replays it through the one function it asks
    /// the kernel with, so a rule cannot be true in the browser and false here.
    const CASES: &str =
        include_str!("../../ds-command-kernel/tests/fixtures/local-layers-cases.json");

    const CLOCK: i64 = 1_758_000_000_000;

    fn scope(lane: &str, uid: &str) -> Scope {
        Scope {
            host: Host::Native,
            lane: Some(lane.into()),
            uid: Some(uid.into()),
        }
    }

    fn roads(dir: &Path, features: usize) -> PathBuf {
        let file = dir.join("roads.geojson");
        let rows: Vec<Value> = (0..features)
            .map(|n| {
                json!({
                    "type": "Feature",
                    "properties": {"n": n},
                    "geometry": {"type": "LineString", "coordinates": [[30.0, -1.9], [30.1, -1.8]]},
                })
            })
            .collect();
        fs::write(
            &file,
            serde_json::to_vec(&json!({"type": "FeatureCollection", "features": rows})).unwrap(),
        )
        .unwrap();
        file
    }

    fn register(name: &str, from: Option<&Path>, now_ms: Option<i64>) -> Op {
        Op::Register(Register {
            name: name.into(),
            geometry: GeometryType::LineString,
            source_name: None,
            from: from.map(Path::to_path_buf),
            now_ms,
        })
    }

    fn catalogue(root: &Path, scope: &Scope) -> PathBuf {
        scope_dir(root, scope).unwrap().join("catalogue.json")
    }

    fn rows(root: &Path, scope: &Scope) -> Vec<Value> {
        let answer = execute_at(root, scope, Op::List { limit: None }).unwrap();
        answer["receipt"]["layers"].as_array().cloned().unwrap()
    }

    // ── the lifecycle ───────────────────────────────────────────────────

    #[test]
    fn a_registered_layer_is_still_there_when_the_store_is_opened_again() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let lane = scope("run", "u1");
        let source = roads(root, 3);

        let admitted = execute_at(
            root,
            &lane,
            register("Access road", Some(&source), Some(CLOCK)),
        )
        .expect("admitted");
        assert_eq!(admitted["receipt"]["id"], "sketch-1758000000000-0");
        assert_eq!(admitted["receipt"]["featureCount"], 3);
        assert_eq!(admitted["receipt"]["sourceKind"], "import");
        assert_eq!(admitted["receipt"]["color"], "#2563eb");
        assert_eq!(admitted["changed"], true);
        assert_eq!(admitted["revision"], 1);
        assert_eq!(admitted["persisted"], PERSISTED);
        assert_eq!(admitted["payload"]["features"], 3);

        // A second call over the same root is a restart: nothing is held in
        // memory between them.
        let listed = rows(root, &lane);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["name"], "Access road");
        assert_eq!(listed[0]["origin"]["kind"], "import_file");
        assert_eq!(listed[0]["origin"]["label"], "roads.geojson");

        let payload = scope_dir(root, &lane)
            .unwrap()
            .join("payloads/sketch-1758000000000-0.geojson");
        assert!(
            payload.is_file(),
            "the copied payload is beside the catalogue"
        );
        assert_eq!(fs::read(&payload).unwrap(), fs::read(&source).unwrap());
        #[cfg(unix)]
        crate::private::assert_private_tree(&scope_dir(root, &lane).unwrap());
    }

    #[test]
    fn a_registered_layer_takes_the_next_colour_and_never_a_used_id() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let lane = scope("run", "u1");
        // The same clock reading twice: the sequence, not the host, keeps the
        // ids apart.
        for n in 0..3 {
            execute_at(
                root,
                &lane,
                register(&format!("Layer {n}"), None, Some(CLOCK)),
            )
            .expect("admitted");
        }
        let listed = rows(root, &lane);
        let ids: Vec<&str> = listed
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            [
                "sketch-1758000000000-0",
                "sketch-1758000000000-1",
                "sketch-1758000000000-2"
            ]
        );
        let answer = execute_at(root, &lane, Op::List { limit: Some(2) }).unwrap();
        assert_eq!(answer["receipt"]["layers"].as_array().unwrap().len(), 2);
        assert_eq!(answer["receipt"]["count"], 3);
    }

    #[test]
    fn a_rename_moves_exactly_what_the_kernel_says_it_moves() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let lane = scope("run", "u1");
        let source = roads(root, 1);

        execute_at(root, &lane, register("Draft poles", None, Some(CLOCK))).unwrap();
        execute_at(
            root,
            &lane,
            register("Imported roads", Some(&source), Some(CLOCK)),
        )
        .unwrap();

        let manual = execute_at(
            root,
            &lane,
            Op::Rename {
                id: "sketch-1758000000000-0".into(),
                name: "Poles".into(),
            },
        )
        .expect("renamed");
        assert_eq!(manual["receipt"]["applied"], "rename");
        assert_eq!(manual["changed"], true);

        let imported = execute_at(
            root,
            &lane,
            Op::Rename {
                id: "sketch-1758000000000-1".into(),
                name: "Trunk roads".into(),
            },
        )
        .expect("renamed");
        assert_eq!(imported["receipt"]["release"], json!([]));

        let listed = rows(root, &lane);
        assert_eq!(listed[0]["name"], "Poles");
        // A manual layer's name inside its source is its name.
        assert_eq!(listed[0]["sourceLayerName"], "Poles");
        assert_eq!(listed[1]["name"], "Trunk roads");
        // An imported layer keeps the name it had inside its file.
        assert_eq!(listed[1]["sourceLayerName"], "Imported roads");
        // Neither rename released a file.
        assert!(scope_dir(root, &lane)
            .unwrap()
            .join("payloads/sketch-1758000000000-1.geojson")
            .is_file());
    }

    #[test]
    fn a_removal_deletes_the_payload_and_leaves_the_original_byte_identical() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let lane = scope("run", "u1");
        let source = roads(root, 4);
        let before = fs::read(&source).unwrap();
        // A second prepared layer with a payload of its own, so the removal
        // has a sibling it must not reach.
        let tracks = root.join("tracks.geojson");
        fs::copy(&source, &tracks).unwrap();

        execute_at(root, &lane, register("Roads", Some(&source), Some(CLOCK))).unwrap();
        execute_at(root, &lane, register("Poles", None, Some(CLOCK))).unwrap();
        execute_at(root, &lane, register("Tracks", Some(&tracks), Some(CLOCK))).unwrap();
        let payloads = payloads_dir(&scope_dir(root, &lane).unwrap());
        let payload = payloads.join("sketch-1758000000000-0.geojson");
        let sibling = payloads.join("sketch-1758000000000-2.geojson");
        assert!(payload.is_file());
        assert!(sibling.is_file());

        let removed = execute_at(
            root,
            &lane,
            Op::Remove {
                id: "sketch-1758000000000-0".into(),
            },
        )
        .expect("removed");
        assert_eq!(removed["receipt"]["removed"], true);
        assert_eq!(
            removed["receipt"]["release"],
            json!([{"kind": "features_payload", "id": "sketch-1758000000000-0"}]),
            "the receipt names the layer's own record and nothing else"
        );
        assert_eq!(
            removed["released"],
            json!([payload.to_string_lossy()]),
            "exactly one file was deleted, and it is the layer's own"
        );

        assert!(!payload.exists(), "the payload copy is gone");
        assert!(sibling.is_file(), "another layer's payload is not reached");
        assert_eq!(fs::read(&sibling).unwrap(), before);
        assert_eq!(
            fs::read(&source).unwrap(),
            before,
            "the file the layer was prepared from is never touched"
        );
        assert!(source.is_file());
        assert!(tracks.is_file());
        let listed = rows(root, &lane);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0]["name"], "Poles");
        assert_eq!(listed[1]["name"], "Tracks");

        // A layer that never had a payload on disk is removed cleanly: the
        // receipt names its record, nothing is there to delete, and that is
        // not a fault.
        let removed = execute_at(
            root,
            &lane,
            Op::Remove {
                id: "sketch-1758000000000-1".into(),
            },
        )
        .expect("removed");
        assert_eq!(
            removed["receipt"]["release"],
            json!([{"kind": "features_payload", "id": "sketch-1758000000000-1"}])
        );
        assert!(
            removed.get("released").is_none(),
            "nothing was on disk to delete"
        );
        assert!(sibling.is_file());
        assert_eq!(rows(root, &lane).len(), 1);
    }

    #[test]
    fn a_row_whose_id_is_not_one_path_segment_refuses_removal_before_any_write() {
        // A hand-edited catalogue can hold an id that would walk out of the
        // payload directory. The kernel now refuses such a catalogue itself —
        // an id is one path segment wherever a catalogue is read — so the
        // refusal arrives before this store names a file at all, and this
        // store keeps its own guard below it either way.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let lane = scope("run", "u1");
        execute_at(root, &lane, register("Poles", None, Some(CLOCK))).unwrap();
        let outside = scope_dir(root, &lane).unwrap().join("escape.geojson");
        fs::write(&outside, b"not a payload").unwrap();

        let file = catalogue(root, &lane);
        let mut registry: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        registry["layers"][0]["id"] = json!("../escape");
        fs::write(&file, serde_json::to_vec(&registry).unwrap()).unwrap();

        unchanged(
            root,
            &lane,
            Op::Remove {
                id: "../escape".into(),
            },
            "malformed_descriptor",
        );
        // The row cannot even be listed: it is the catalogue that is refused,
        // so the store never reaches a file for it.
        let listed = execute_at(root, &lane, Op::List { limit: None }).expect_err("must refuse");
        assert_eq!(listed.code(), "malformed_descriptor", "{listed:?}");
        assert_eq!(
            fs::read(&outside).unwrap(),
            b"not a payload",
            "a file outside the payload directory is never reached"
        );
        // And the guard this store holds on its own, below the kernel's.
        assert!(payload_path(&scope_dir(root, &lane).unwrap(), "../escape").is_err());
    }

    // ── refusals change nothing ─────────────────────────────────────────

    /// Every refusal below asserts the same two things: the typed code, and
    /// that the catalogue on disk is the same bytes it was before.
    fn unchanged(root: &Path, lane: &Scope, op: Op, code: &str) {
        let file = catalogue(root, lane);
        let before = fs::read(&file).ok();
        let error = execute_at(root, lane, op).expect_err("must refuse");
        assert_eq!(error.code(), code, "{error:?}");
        assert_eq!(fs::read(&file).ok(), before, "a refusal rewrote the store");
    }

    #[test]
    fn an_unknown_layer_refuses_and_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let lane = scope("run", "u1");
        execute_at(root, &lane, register("Poles", None, Some(CLOCK))).unwrap();

        unchanged(
            root,
            &lane,
            Op::Remove {
                id: "sketch-nope".into(),
            },
            "unknown_layer",
        );
        unchanged(
            root,
            &lane,
            Op::Rename {
                id: "sketch-nope".into(),
                name: "Poles".into(),
            },
            "unknown_layer",
        );
        assert_eq!(rows(root, &lane).len(), 1);
    }

    #[test]
    fn a_catalogue_holding_one_id_twice_refuses_every_op_and_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let lane = scope("run", "u1");
        execute_at(root, &lane, register("Poles", None, Some(CLOCK))).unwrap();

        // A hand-edited catalogue that repeats an id: the kernel refuses it,
        // and the store never quietly drops the second row to make it read.
        let file = catalogue(root, &lane);
        let mut registry: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        let row = registry["layers"][0].clone();
        registry["layers"].as_array_mut().unwrap().push(row);
        fs::write(&file, serde_json::to_vec(&registry).unwrap()).unwrap();

        unchanged(root, &lane, Op::List { limit: None }, "duplicate_layer");
        unchanged(
            root,
            &lane,
            register("Another", None, Some(CLOCK)),
            "duplicate_layer",
        );
        unchanged(
            root,
            &lane,
            Op::Remove {
                id: "sketch-1758000000000-0".into(),
            },
            "duplicate_layer",
        );
    }

    #[test]
    fn a_blank_or_oversized_name_refuses_and_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let lane = scope("run", "u1");
        execute_at(root, &lane, register("Poles", None, Some(CLOCK))).unwrap();

        unchanged(
            root,
            &lane,
            register("   ", None, Some(CLOCK)),
            "malformed_descriptor",
        );
        unchanged(
            root,
            &lane,
            register(&"n".repeat(201), None, Some(CLOCK)),
            "malformed_descriptor",
        );
        unchanged(
            root,
            &lane,
            Op::Rename {
                id: "sketch-1758000000000-0".into(),
                name: " ".into(),
            },
            "malformed_descriptor",
        );
    }

    #[test]
    fn a_catalogue_that_belongs_to_another_lane_or_account_is_never_rewritten() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let mine = scope("run", "u1");
        execute_at(root, &mine, register("Poles", None, Some(CLOCK))).unwrap();

        // Another account's store is its own: empty, and reading it creates
        // nothing.
        let theirs = scope("run", "u2");
        assert!(rows(root, &theirs).is_empty());
        assert!(!scope_dir(root, &theirs).unwrap().exists());
        let other_lane = scope("canary", "u1");
        assert!(rows(root, &other_lane).is_empty());
        assert_eq!(rows(root, &mine).len(), 1);

        // A catalogue whose own recorded scope is somebody else's refuses
        // every op rather than being adopted.
        let file = catalogue(root, &mine);
        let mut registry: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        registry["scope"]["uid"] = json!("u2");
        fs::write(&file, serde_json::to_vec(&registry).unwrap()).unwrap();

        unchanged(root, &mine, Op::List { limit: None }, "scope_mismatch");
        unchanged(
            root,
            &mine,
            register("Another", None, Some(CLOCK)),
            "scope_mismatch",
        );
        unchanged(
            root,
            &mine,
            Op::Remove {
                id: "sketch-1758000000000-0".into(),
            },
            "scope_mismatch",
        );
    }

    #[test]
    fn a_malformed_catalogue_is_refused_and_never_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let lane = scope("run", "u1");
        let dir = scope_dir(root, &lane).unwrap();
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("catalogue.json");
        fs::write(&file, b"{not json").unwrap();
        fs::write(dir.join("catalogue.lock"), b"").unwrap();

        for op in [
            Op::List { limit: None },
            register("Poles", None, Some(CLOCK)),
            Op::Rename {
                id: "sketch-1758000000000-0".into(),
                name: "Poles".into(),
            },
            Op::Remove {
                id: "sketch-1758000000000-0".into(),
            },
        ] {
            let error = execute_at(root, &lane, op).expect_err("must refuse");
            assert_eq!(error.code(), "local_layer_refused");
            assert!(error.message().contains("malformed"), "{error}");
        }
        assert_eq!(fs::read(&file).unwrap(), b"{not json");
    }

    #[test]
    fn a_store_that_was_never_opened_reads_as_empty_and_creates_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("never-opened");
        let lane = scope("run", "u1");
        let answer = execute_at(&root, &lane, Op::List { limit: None }).unwrap();
        assert_eq!(answer["receipt"]["layers"], json!([]));
        assert_eq!(answer["receipt"]["count"], 0);
        assert_eq!(answer["changed"], false);
        assert!(!root.exists(), "a read created a directory");
    }

    /// Every path under a root, relative and sorted. "The store was not
    /// touched" is a claim about files that must not be there, so it is
    /// asserted over the list itself rather than over one expected name.
    fn entries(root: &Path) -> Vec<String> {
        let mut found = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            let Ok(listing) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in listing.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path.clone());
                }
                found.push(
                    path.strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        found.sort();
        found
    }

    #[test]
    fn a_refused_op_never_opens_a_store_that_was_never_opened() {
        // The kernel is asked over the catalogue as it was READ, so a refused
        // write answers before this host has created anything: no lane
        // directory, no catalogue.lock, no catalogue, and — for a register —
        // no copied payload either. The root is listed before and after each
        // refusal, so a stray file of any name would show.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let lane = scope("run", "u1");
        let source = roads(root, 2);
        let before = entries(root);
        assert_eq!(before, vec!["roads.geojson".to_owned()]);

        let refusals = || {
            vec![
                (
                    Op::Remove {
                        id: "sketch-nope".into(),
                    },
                    "unknown_layer",
                ),
                (
                    Op::Rename {
                        id: "sketch-nope".into(),
                        name: "Trunk roads".into(),
                    },
                    "unknown_layer",
                ),
                // A real file, admitted as a payload, and a name the kernel
                // refuses: the bytes were read but nothing was written.
                (
                    register("   ", Some(&source), Some(CLOCK)),
                    "malformed_descriptor",
                ),
                (
                    register(&"n".repeat(201), None, Some(CLOCK)),
                    "malformed_descriptor",
                ),
                // And the refusal that never reaches the kernel at all.
                (
                    Op::Register(Register {
                        name: "Wrong geometry".into(),
                        geometry: GeometryType::Point,
                        source_name: None,
                        from: Some(source.clone()),
                        now_ms: Some(CLOCK),
                    }),
                    "invalid_payload",
                ),
            ]
        };

        for (op, code) in refusals() {
            let error = execute_at(root, &lane, op).expect_err("must refuse");
            assert_eq!(error.code(), code, "{error:?}");
            assert_eq!(entries(root), before, "a refusal opened the store");
        }
        assert!(
            !scope_dir(root, &lane).unwrap().exists(),
            "a refusal created the lane and account's directory"
        );

        // Once a write has been admitted the store exists, and the same
        // refusals leave exactly the files that write left — no extra lock, no
        // orphan payload, no half-written catalogue.
        execute_at(root, &lane, register("Poles", None, Some(CLOCK))).expect("admitted");
        let opened = entries(root);
        assert!(opened.iter().any(|path| path.ends_with("catalogue.json")));
        for (op, code) in refusals() {
            let error = execute_at(root, &lane, op).expect_err("must refuse");
            assert_eq!(error.code(), code, "{error:?}");
            assert_eq!(entries(root), opened, "a refusal left something behind");
        }
        assert_eq!(rows(root, &lane).len(), 1);
    }

    // ── the payload boundary ────────────────────────────────────────────

    #[test]
    fn only_a_bounded_feature_collection_of_the_declared_geometry_is_copied() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let lane = scope("run", "u1");
        let write = |name: &str, body: Value| {
            let file = root.join(name);
            fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
            file
        };
        let point = json!({"type": "Feature", "properties": {},
            "geometry": {"type": "Point", "coordinates": [30.0, -1.9]}});
        let bad = [
            write("not-json.geojson", json!("{")),
            write("bare.geojson", json!({"type": "Feature"})),
            write(
                "wrong-geometry.geojson",
                json!({"type": "FeatureCollection", "features": [point]}),
            ),
            write(
                "no-geometry.geojson",
                json!({"type": "FeatureCollection", "features": [{"type": "Feature"}]}),
            ),
            write(
                "not-a-feature.geojson",
                json!({"type": "FeatureCollection", "features": [{"type": "Thing"}]}),
            ),
            root.join("absent.geojson"),
            root.to_path_buf(),
        ];
        for file in &bad {
            let error = execute_at(root, &lane, register("Roads", Some(file), Some(CLOCK)))
                .expect_err("must refuse");
            assert_eq!(error.code(), "invalid_payload", "{file:?} was admitted");
        }
        assert!(
            !scope_dir(root, &lane).unwrap().exists(),
            "a refused payload opened a store"
        );

        // A MultiLineString belongs to a LineString layer; one paint covers
        // both. An empty collection is a layer with no features yet.
        let multi = write(
            "multi.geojson",
            json!({"type": "FeatureCollection", "features": [{"type": "Feature",
                "geometry": {"type": "MultiLineString", "coordinates": [[[30.0, -1.9], [30.1, -1.8]]]}}]}),
        );
        let empty = write(
            "empty.geojson",
            json!({"type": "FeatureCollection", "features": []}),
        );
        assert_eq!(
            execute_at(root, &lane, register("Multi", Some(&multi), Some(CLOCK))).unwrap()
                ["receipt"]["featureCount"],
            1
        );
        assert_eq!(
            execute_at(root, &lane, register("Empty", Some(&empty), Some(CLOCK))).unwrap()
                ["receipt"]["featureCount"],
            0
        );
    }

    #[test]
    fn a_file_carrying_a_geometry_ds_does_not_admit_is_refused_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let lane = scope("run", "u1");
        let line = |id: &str, coordinates: Value| {
            json!({"type": "Feature", "id": id, "properties": {},
                "geometry": {"type": "LineString", "coordinates": coordinates}})
        };
        let file = root.join("roads.geojson");
        fs::write(
            &file,
            serde_json::to_vec(&json!({"type": "FeatureCollection", "features": [
                line("r1", json!([[30.0, -1.9, 1500.0], [30.1, -1.8, 1490.0]])),
                line("r2", json!([[30.0, -1.9]])),
                line("r3", json!([[30.0, -95.0], [30.1, -1.8]])),
            ]}))
            .unwrap(),
        )
        .unwrap();
        let error = execute_at(root, &lane, register("Roads", Some(&file), Some(CLOCK)))
            .expect_err("must refuse");
        assert_eq!(error.code(), "invalid_payload");
        let message = error.to_string();
        assert!(
            message.contains(
                "roads.geojson geometry_invalid: 2 of 3 features carry a geometry DS does not \
                 admit: feature 2 (id \"r2\") geometry_invalid: LineString has 1 position; a \
                 line needs at least 2; feature 3 (id \"r3\") geometry_invalid: LineString \
                 position 0 has latitude -95, outside [-90, 90]; nothing was prepared"
            ),
            "{message}"
        );
        assert!(
            !scope_dir(root, &lane).unwrap().exists(),
            "a refused payload opened a store"
        );
    }

    #[test]
    fn a_payload_is_copied_verbatim_and_the_original_is_only_ever_read() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let lane = scope("run", "u1");
        // Whitespace and key order a re-serialization would not preserve.
        let source = root.join("hand-written.geojson");
        let text = b"{ \"type\":\"FeatureCollection\",\n  \"features\" : [ ] }\n";
        fs::write(&source, text).unwrap();
        let permissions = fs::metadata(&source).unwrap().permissions();

        let admitted =
            execute_at(root, &lane, register("Hand", Some(&source), Some(CLOCK))).unwrap();
        let copied = admitted["payload"]["path"].as_str().unwrap();
        assert_eq!(
            fs::read(copied).unwrap(),
            text,
            "the copy is the same bytes"
        );
        assert_eq!(
            fs::read(&source).unwrap(),
            text,
            "the original is untouched"
        );
        assert_eq!(fs::metadata(&source).unwrap().permissions(), permissions);
        assert_eq!(
            admitted["receipt"]["sourceMetadata"]["sourcePath"],
            json!(std::path::absolute(&source).unwrap().to_string_lossy()),
            "the origin is recorded as provenance"
        );
    }

    #[test]
    fn an_id_or_a_scope_that_is_not_one_path_segment_names_nothing_here() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for bad in ["../escape", "a/b", "..", ".", "", " lane", "c:drive"] {
            assert!(!safe_segment(bad), "`{bad}` was accepted as a path segment");
            assert!(payload_path(root, bad).is_err());
            for pair in [
                Scope {
                    host: Host::Native,
                    lane: Some(bad.into()),
                    uid: Some("u1".into()),
                },
                Scope {
                    host: Host::Native,
                    lane: Some("run".into()),
                    uid: Some(bad.into()),
                },
            ] {
                assert!(scope_dir(root, &pair).is_err(), "`{bad}` named a directory");
            }
        }
        assert!(safe_segment("run") && safe_segment("sketch-1758000000000-0"));
        // A browser profile keeps its catalogue in IndexedDB; this store never
        // speaks for one.
        assert!(scope_dir(
            root,
            &Scope {
                host: Host::Browser,
                lane: None,
                uid: None
            }
        )
        .is_err());
    }

    #[test]
    fn the_catalogue_is_bounded_on_write_as_well_as_on_read() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let file = dir.join("catalogue.json");
        fs::write(&file, b"kept").unwrap();
        let mut registry = opened(&scope("run", "u1"));
        let row: LocalLayer = serde_json::from_value(json!({
            "id": "sketch-1",
            "name": "n".repeat(200),
            "sourceName": "s".repeat(400),
            "sourceLayerName": "l".repeat(400),
            "sourceKind": "manual",
            "geometryType": "Point",
            "color": "#2563eb",
            "lineWidth": 2.5,
            "pointRadius": 5.0,
            "fillOpacity": 0.15,
            "schema": [],
            "featureCount": 0,
            "visible": true,
            "createdAt": CLOCK,
        }))
        .unwrap();
        for n in 0..1_200 {
            let mut row = row.clone();
            row.id = format!("sketch-{n}");
            registry.layers.push(row);
        }
        let error = persist(&file, dir, &registry).expect_err("must refuse");
        assert!(error.message().contains("1 MiB"), "{error}");
        assert_eq!(
            fs::read(&file).unwrap(),
            b"kept",
            "a store that cannot be reopened is never written"
        );
    }

    // ── concurrency ─────────────────────────────────────────────────────

    #[test]
    fn concurrent_writers_on_one_catalogue_each_keep_their_row() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let lane = scope("run", "u1");
        let shared = &lane;
        std::thread::scope(|threads| {
            for n in 0..12 {
                threads.spawn(move || {
                    execute_at(
                        root,
                        shared,
                        register(&format!("Layer {n}"), None, Some(CLOCK)),
                    )
                    .expect("admitted");
                });
            }
        });
        let listed = rows(root, &lane);
        assert_eq!(listed.len(), 12);
        let ids: BTreeSet<&str> = listed
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids.len(), 12, "two writers minted the same id");
        let names: BTreeSet<&str> = listed
            .iter()
            .map(|row| row["name"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), 12);
    }

    // ── the recorded corpus ─────────────────────────────────────────────

    fn corpus() -> Value {
        let corpus: Value = serde_json::from_str(CASES).expect("cases parse");
        assert_eq!(corpus["schema"], "ds.local-layers-cases/v1");
        corpus
    }

    #[test]
    fn the_store_asks_the_kernel_the_recorded_corpus_answers() {
        let corpus = corpus();
        let cases = corpus["cases"].as_array().expect("cases");
        // The re-recorded corpus (70 cases after the gis_folder cases left on
        // 2026-10-20), the kernel's fixing review included: the
        // cases its new rules answer — a repaired source label, a pinned pair,
        // a fenced rename, a traversal-shaped id, the metadata rule on every
        // op — are replayed here too, not only the shape that came before.
        assert!(cases.len() >= 70, "the corpus lost cases");
        let mut drift = Vec::new();
        for case in cases {
            let id = case["id"].as_str().expect("case id");
            let expected = &case["expected"];
            let schema = case
                .get("schema")
                .and_then(Value::as_str)
                .unwrap_or(kernel::SCHEMA);
            let actual = match ask_as(schema, case["request"].clone()) {
                Ok(answer) => answer,
                Err(StoreError::Refused { code, message }) => {
                    json!({"outcome": "refused", "refusal": {"code": code, "message": message}})
                }
                Err(error) => json!({"error": error.message()}),
            };
            if &actual != expected {
                drift.push(format!(
                    "{id}:\n  recorded: {expected}\n  store:    {actual}"
                ));
            }
        }
        assert!(drift.is_empty(), "\n{}", drift.join("\n\n"));
    }

    #[test]
    fn a_refused_case_never_answers_a_registry_the_store_could_write() {
        // The store persists `answer["registry"]`, so this is the property
        // that makes "a refusal changes nothing" true HERE and not only in
        // the kernel's own test.
        for case in corpus()["cases"].as_array().expect("cases") {
            let id = case["id"].as_str().expect("case id");
            let schema = case
                .get("schema")
                .and_then(Value::as_str)
                .unwrap_or(kernel::SCHEMA);
            match ask_as(schema, case["request"].clone()) {
                Ok(answer) => assert!(
                    answer["registry"]["layers"].is_array(),
                    "{id}: an admitted answer carries the next catalogue"
                ),
                Err(error) => assert!(
                    matches!(error, StoreError::Refused { .. } | StoreError::Store(_)),
                    "{id}: a refusal is typed"
                ),
            }
        }
    }

    #[test]
    fn a_release_in_the_corpus_only_ever_names_a_file_this_store_owns() {
        // What `release()` is allowed to delete, asserted over every recorded
        // receipt rather than over prose: an id under this store's own payload
        // directory, and nothing that resolves anywhere else.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        for case in corpus()["cases"].as_array().expect("cases") {
            let id = case["id"].as_str().expect("case id");
            for item in case["expected"]["receipt"]["release"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let kind = item["kind"].as_str().unwrap_or_default();
                assert!(
                    matches!(kind, "features_payload" | "upload_chunks"),
                    "{id}: {kind} is not a record a local layer owns"
                );
                if kind != "features_payload" {
                    continue;
                }
                let named = item["id"].as_str().expect("a released record has an id");
                let file = payload_path(dir, named).expect("one path segment");
                assert!(
                    file.starts_with(payloads_dir(dir)),
                    "{id}: {named} left the payload directory"
                );
            }
        }
    }

    #[test]
    fn every_id_a_recorded_answer_admits_is_one_this_store_can_own_a_file_for() {
        // The kernel mints ids for layers this store never registers itself: a
        // pinned row is named by the transformer and class it was materialised
        // from, not by a clock and a counter. Whatever shape such an id takes,
        // a native catalogue holding that row has to be able to name its
        // payload — so every id an admitted answer carries must resolve here,
        // and inside this store's own payload directory.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let mut seen = 0;
        for case in corpus()["cases"].as_array().expect("cases") {
            let case_id = case["id"].as_str().expect("case id");
            for layer in case["expected"]["registry"]["layers"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let Some(named) = layer["id"].as_str() else {
                    continue;
                };
                let file = payload_path(dir, named).unwrap_or_else(|error| {
                    panic!("{case_id}: the store cannot own a payload for {named}: {error}")
                });
                assert!(
                    file.starts_with(payloads_dir(dir)),
                    "{case_id}: {named} left the payload directory"
                );
                seen += 1;
            }
        }
        assert!(seen > 0, "the corpus admitted no layer at all");
    }

    #[test]
    fn a_recorded_native_case_replays_over_a_real_catalogue_file() {
        // The corpus' native register case, driven through the file store
        // rather than through `ask`: the same descriptor, the same answer, and
        // the bytes on disk are the registry the kernel returned.
        let case = corpus()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["id"] == "register-into-a-native-store")
            .expect("the native register case")
            .clone();
        let request = &case["request"];
        let lane = scope(
            request["scope"]["lane"].as_str().unwrap(),
            request["scope"]["uid"].as_str().unwrap(),
        );
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let admitted = execute_at(
            root,
            &lane,
            Op::Register(Register {
                name: request["draft"]["name"].as_str().unwrap().into(),
                geometry: geometry_type(request["draft"]["geometryType"].as_str().unwrap())
                    .unwrap(),
                source_name: None,
                from: None,
                now_ms: request["now_ms"].as_i64(),
            }),
        )
        .expect("admitted");

        // The sequence is the one counter this host owns, so a fresh store
        // mints `-0` where the recorded case passed 1. Everything the kernel
        // decided — the label, the colour, every default — is the recorded
        // descriptor exactly.
        let minted = admitted["receipt"]["id"].as_str().unwrap().to_owned();
        assert_eq!(minted, "sketch-1758000000000-0");
        let mut expected = case["expected"]["receipt"].clone();
        expected["id"] = json!(minted);
        assert_eq!(admitted["receipt"], expected);
        assert_eq!(admitted["changed"], case["expected"]["changed"]);

        let written: Value =
            serde_json::from_slice(&fs::read(catalogue(root, &lane)).unwrap()).unwrap();
        let mut recorded = case["expected"]["registry"].clone();
        recorded["layers"][0] = expected;
        assert_eq!(
            written, recorded,
            "the file holds exactly the registry the kernel answered"
        );
    }

    #[test]
    fn the_store_reads_the_same_corpus_the_browser_replays() {
        let web = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../src/tests/fixtures/local-layers/cases.json");
        let Ok(shared) = fs::read_to_string(&web) else {
            return;
        };
        assert_eq!(
            shared, CASES,
            "ds-web's copy of the corpus has drifted from the kernel's"
        );
    }

    #[test]
    fn the_geometry_words_the_descriptor_spells_are_the_ones_that_parse() {
        for (word, kind) in [
            ("point", GeometryType::Point),
            ("Point", GeometryType::Point),
            ("linestring", GeometryType::LineString),
            ("LineString", GeometryType::LineString),
            ("polygon", GeometryType::Polygon),
        ] {
            assert_eq!(geometry_type(word), Some(kind));
        }
        assert_eq!(geometry_type("multipolygon"), None);
        assert_eq!(geometry_type(""), None);
    }

    #[test]
    fn the_request_the_store_builds_carries_only_clocks_and_counters() {
        // Nothing in a request is a decision: the kernel receives the
        // catalogue, the scope, a clock, a sequence and a palette cursor, and
        // the two words the caller typed.
        let registry = json!({"schema": 1, "revision": 0, "scope": {"host": "native",
            "lane": "run", "uid": "u1"}, "layers": []});
        let registry: Registry = serde_json::from_value(registry).unwrap();
        let op = request(
            &register("Roads", None, Some(CLOCK)),
            &registry,
            &scope("run", "u1"),
            None,
        )
        .unwrap();
        let keys: Vec<&str> = op
            .as_object()
            .map(Map::keys)
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect();
        assert!(
            keys.iter().collect::<BTreeSet<_>>()
                == BTreeSet::from([
                    &"kind",
                    &"registry",
                    &"scope",
                    &"now_ms",
                    &"sequence",
                    &"colorIndex",
                    &"draft"
                ]),
            "the request grew a field: {keys:?}"
        );
        assert_eq!(
            op["draft"],
            json!({"name": "Roads", "geometryType": "LineString"})
        );
        assert_eq!(op["now_ms"], CLOCK);
        assert_eq!(op["sequence"], 0);
        assert_eq!(op["colorIndex"], 0);
    }
}
