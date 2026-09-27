//! Shared native storage for immutable reference query pages. This is a response
//! cache, separate from canonical GeoParquet assets and derived DSAB indexes.
use crate::private;
use ds_command_kernel::geometry_admission;
use ds_command_kernel::reference_cache::{
    self as contract, Identity, Page, MAX_PAGES, MAX_PAGE_BYTES, MAX_RESOURCE_BYTES, PAGE_ROWS,
};
use fs2::FileExt;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const INDEX_FILE: &str = "spatial-index.sqlite";
const BUNDLE_INDEX_FILE: &str = "bundle-index.sqlite";
/// The index under construction. Named by this code, carrying the extension
/// SQLite's own tooling expects, and never left behind: a half-built index that
/// survived a crash is removed before the next build rather than reopened.
const INDEX_BUILD_FILE: &str = "spatial-index.building.sqlite";
const LOCATION_FILE: &str = "reference-query-cache-location.json";
const OWNED_ROOT: &str = "rw.datasolutions.desktop.shared";

#[derive(Debug)]
struct CacheSummary {
    features: usize,
    bytes: usize,
    terminal: bool,
    message: String,
}

/// How a bundle refusal for a feature's geometry begins: a bundle holding one
/// feature DS does not admit (`ds_command_kernel::geometry_admission`) is
/// refused whole, and the message names that feature.
pub const BUNDLE_INVALID: &str = "bundle_invalid";

/// Refuse a reference feature whose geometry DS does not admit, naming it by
/// its 1-based `number` in the resource. A feature without geometry passes.
fn admit_reference_feature(number: usize, feature: &Value) -> Result<(), String> {
    if feature["geometry"].is_null() {
        return Ok(());
    }
    geometry_admission::admit_geometry(&feature["geometry"]).map_err(|refusal| {
        let id = match &feature["id"] {
            Value::String(id) => format!(" (id {id:?})"),
            Value::Number(id) => format!(" (id {id})"),
            _ => String::new(),
        };
        format!("{BUNDLE_INVALID}: feature {number}{id} {refusal}")
    })
}

/// Native bundle ingestion is not cloud response pagination. A thousand large
/// polygons can exceed a response's byte ceiling. Stream features into SQLite
/// instead, and commit the metadata only after the caller verifies the complete
/// expanded bundle digest. The caller must use a private staging root.
pub struct BundleIndexWriter {
    connection: Connection,
    identity: Identity,
    features: usize,
}

impl BundleIndexWriter {
    pub fn new(staging_root: &Path, identity: Identity) -> Result<Self, String> {
        let dir = directory(staging_root, &identity)?;
        let path = dir.join(BUNDLE_INDEX_FILE);
        if path.exists() {
            return Err("bundle index staging destination already exists".into());
        }
        let connection = open_index_for_write(&path)?;
        connection.execute_batch("CREATE TABLE metadata(id INTEGER PRIMARY KEY CHECK(id=1), resource_id TEXT NOT NULL, version TEXT NOT NULL, feature_count INTEGER NOT NULL, source_bytes INTEGER NOT NULL);
            CREATE TABLE features(id INTEGER PRIMARY KEY, feature_json TEXT NOT NULL);
            CREATE VIRTUAL TABLE feature_rtree USING rtree(id,min_x,max_x,min_y,max_y);
            BEGIN IMMEDIATE;").map_err(|e| e.to_string())?;
        Ok(Self {
            connection,
            identity,
            features: 0,
        })
    }
    /// Index one bundle feature. A geometry DS does not admit refuses it with
    /// [`BUNDLE_INVALID`] naming the feature; the caller discards the staging
    /// index, so a bundle carrying one is never indexed at all.
    pub fn push(&mut self, feature: &Value) -> Result<(), String> {
        if feature["type"] != "Feature"
            || !feature["properties"].is_object()
            || !(feature["geometry"].is_object() || feature["geometry"].is_null())
        {
            return Err("invalid bundle feature".into());
        }
        admit_reference_feature(self.features + 1, feature)?;
        let json = serde_json::to_string(feature).map_err(|e| e.to_string())?;
        self.connection
            .prepare_cached("INSERT INTO features(id,feature_json) VALUES (?1,?2)")
            .map_err(|e| e.to_string())?
            .execute(params![self.features as i64, json])
            .map_err(|e| e.to_string())?;
        if let Some(b) = contract::feature_bounds(feature) {
            self.connection
                .prepare_cached(
                    "INSERT INTO feature_rtree(id,min_x,max_x,min_y,max_y) VALUES (?1,?2,?3,?4,?5)",
                )
                .map_err(|e| e.to_string())?
                .execute(params![self.features as i64, b[0], b[2], b[1], b[3]])
                .map_err(|e| e.to_string())?;
        }
        self.features += 1;
        Ok(())
    }
    pub fn finish(self, verified_source_bytes: u64) -> Result<(), String> {
        if self.features == 0
            || verified_source_bytes == 0
            || verified_source_bytes > MAX_RESOURCE_BYTES
        {
            return Err("invalid verified bundle size".into());
        }
        self.connection
            .execute(
                "INSERT INTO metadata VALUES(1,?1,?2,?3,?4)",
                params![
                    self.identity.resource_id,
                    self.identity.version,
                    self.features as i64,
                    verified_source_bytes
                ],
            )
            .map_err(|e| e.to_string())?;
        self.connection
            .execute_batch("COMMIT; PRAGMA optimize;")
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Open one of this store's SQLite databases for writing, at a path length
/// Windows accepts, with journalling off before anything is written.
///
/// `journal_mode=OFF` is set on the connection before the first statement, not
/// beside the schema: a journal or WAL sidecar is a second file in the same
/// over-long directory, and a build that is discarded on failure has nothing to
/// roll back to anyway — the whole database is thrown away.
fn open_index_for_write(path: &Path) -> Result<Connection, String> {
    private::prepare_file(path).map_err(|error| format!("open reference index: {error}"))?;
    let connection = Connection::open_with_flags(
        crate::sqlite_path(path)?,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
    )
    .map_err(|error| format!("open reference index: {error}"))?;
    connection
        .execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=FULL;")
        .map_err(|error| format!("prepare reference index: {error}"))?;
    Ok(connection)
}

/// Open one of this store's SQLite databases read-only, at a path length
/// Windows accepts.
fn open_index_for_read(path: &Path) -> Result<Connection, rusqlite::Error> {
    let path = crate::sqlite_path(path)
        .map_err(|error| rusqlite::Error::InvalidPath(PathBuf::from(error)))?;
    Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
}

fn completed_bundle(dir: &Path, identity: &Identity) -> Option<CacheSummary> {
    let md = fs::symlink_metadata(dir.join(BUNDLE_INDEX_FILE)).ok()?;
    if linked(&md) || !md.is_file() {
        return None;
    }
    let connection = open_index_for_read(&dir.join(BUNDLE_INDEX_FILE)).ok()?;
    let (resource, version, features, bytes): (String, String, usize, usize) = connection
        .query_row(
            "SELECT resource_id,version,feature_count,source_bytes FROM metadata WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .ok()?;
    if resource != identity.resource_id
        || version != identity.version
        || features == 0
        || bytes == 0
    {
        return None;
    }
    Some(CacheSummary {
        features,
        bytes,
        terminal: true,
        message: String::new(),
    })
}

fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}
fn directory(root: &Path, identity: &Identity) -> Result<PathBuf, String> {
    identity.validate()?;
    let mut current = root.to_path_buf();
    private::create_dir_all(&current).map_err(|e| e.to_string())?;
    if linked(&fs::symlink_metadata(&current).map_err(|e| e.to_string())?) {
        return Err("linked reference root is not allowed".into());
    }
    for segment in [
        "reference-query-cache",
        "v2",
        &identity.resource_id,
        &identity.version,
    ] {
        current.push(segment);
        if current.exists() {
            let md = fs::symlink_metadata(&current).map_err(|e| e.to_string())?;
            if linked(&md) || !md.is_dir() {
                return Err("linked or invalid reference cache directory".into());
            }
        } else {
            match private::create_dir(&current) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(e.to_string()),
            };
            let md = fs::symlink_metadata(&current).map_err(|e| e.to_string())?;
            if linked(&md) || !md.is_dir() {
                return Err("invalid reference directory".into());
            }
        }
    }
    Ok(current)
}
fn read_page(dir: &Path, identity: &Identity, offset: usize) -> Result<Page, String> {
    let path = dir.join(format!("{offset}.json"));
    let md = fs::symlink_metadata(&path)
        .map_err(|_| "reference cache is incomplete; download this resource")?;
    if linked(&md) || !md.is_file() || md.len() > (2 * MAX_PAGE_BYTES) as u64 {
        return Err("invalid reference cache file".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .map_err(|e| e.to_string())?
        .take((2 * MAX_PAGE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let page: Page = serde_json::from_slice(&bytes).map_err(|_| "invalid reference cache page")?;
    contract::validate_page(identity, offset, &page)?;
    Ok(page)
}

fn scan_pages(dir: &Path, identity: &Identity) -> Result<CacheSummary, String> {
    let mut features = 0usize;
    let mut bytes = 0usize;
    for index in 0..MAX_PAGES {
        let page = match read_page(dir, identity, index * PAGE_ROWS) {
            Ok(page) => page,
            Err(error) => {
                return Ok(CacheSummary {
                    features,
                    bytes,
                    terminal: false,
                    message: error,
                });
            }
        };
        features = features.saturating_add(page.feature_count);
        bytes = bytes.saturating_add(page.feature_json.len());
        if bytes as u64 > MAX_RESOURCE_BYTES {
            return Err("reference cache exceeds resource limit".into());
        }
        if page.next_offset.is_none() {
            return Ok(CacheSummary {
                features,
                bytes,
                terminal: true,
                message: String::new(),
            });
        }
    }
    Err("reference cache page limit exceeded".into())
}

fn index_valid(dir: &Path, identity: &Identity, summary: &CacheSummary) -> bool {
    completed_summary(dir, identity)
        .is_some_and(|held| held.features == summary.features && held.bytes == summary.bytes)
}

// Installation hashes every page before building this immutable index. Ordinary
// reads check its identity and page metadata, not every feature in the country.
// Candidate pages are still hash-validated when a spatial query reads them.
fn completed_summary(dir: &Path, identity: &Identity) -> Option<CacheSummary> {
    if let Some(summary) = completed_bundle(dir, identity) {
        return Some(summary);
    }
    let md = fs::symlink_metadata(dir.join(INDEX_FILE)).ok()?;
    if linked(&md) || !md.is_file() {
        return None;
    }
    let Ok(connection) = open_index_for_read(&dir.join(INDEX_FILE)) else {
        return None;
    };
    let row: rusqlite::Result<(String, String, i64, i64)> = connection.query_row(
        "SELECT resource_id, version, feature_count, source_bytes FROM metadata WHERE id = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    );
    let (resource, version, features, bytes) = row.ok()?;
    if resource != identity.resource_id || version != identity.version {
        return None;
    }
    let mut statement = connection
        .prepare("SELECT page_offset, size, modified FROM page_manifest ORDER BY page_offset")
        .ok()?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, usize>(0)?,
                row.get::<_, u64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .ok()?;
    let mut count = 0;
    for row in rows {
        let (offset, size, modified) = row.ok()?;
        let stamp = page_stamp(dir, offset).ok()?;
        if offset != count * PAGE_ROWS || stamp != (size, modified) {
            return None;
        }
        count += 1;
    }
    let features = usize::try_from(features).ok()?;
    if count != features.max(1).div_ceil(PAGE_ROWS) {
        return None;
    }
    Some(CacheSummary {
        features,
        bytes: usize::try_from(bytes).ok()?,
        terminal: true,
        message: String::new(),
    })
}

fn page_stamp(dir: &Path, offset: usize) -> Result<(u64, String), String> {
    let md = fs::symlink_metadata(dir.join(format!("{offset}.json"))).map_err(|e| e.to_string())?;
    if linked(&md) || !md.is_file() {
        return Err("invalid reference page".into());
    }
    let modified = md
        .modified()
        .map_err(|e| e.to_string())?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?;
    Ok((md.len(), modified.as_nanos().to_string()))
}

fn build_index(dir: &Path, identity: &Identity, summary: &CacheSummary) -> Result<(), String> {
    if !summary.terminal {
        return Err("reference cache is incomplete; download this resource".into());
    }
    // An explicit file name this code chose, with a `.sqlite` extension, rather
    // than a `NamedTempFile`'s: the database is opened by name a second time by
    // SQLite itself (and by a journal or sidecar if one were ever enabled), so
    // the name has to be one this store owns and can clean up, not one a temp
    // handle owns for as long as it is alive.
    let temp_path = dir.join(INDEX_BUILD_FILE);
    match fs::remove_file(&temp_path) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.to_string()),
    }
    let built = (|| -> Result<(), String> {
        let mut connection = open_index_for_write(&temp_path)?;
        connection
            .execute_batch(
                "CREATE TABLE metadata (
                   id INTEGER PRIMARY KEY CHECK (id = 1),
                   resource_id TEXT NOT NULL,
                   version TEXT NOT NULL,
                   feature_count INTEGER NOT NULL,
                   source_bytes INTEGER NOT NULL
                 );
                 CREATE TABLE feature_locations (
                   id INTEGER PRIMARY KEY,
                   page_offset INTEGER NOT NULL,
                   feature_index INTEGER NOT NULL
                 );
                 CREATE TABLE page_manifest (
                   page_offset INTEGER PRIMARY KEY,
                   size INTEGER NOT NULL,
                   modified TEXT NOT NULL
                 );
                 CREATE VIRTUAL TABLE feature_rtree USING rtree(
                   id, min_x, max_x, min_y, max_y
                 );",
            )
            .map_err(|e| e.to_string())?;
        let transaction = connection.transaction().map_err(|e| e.to_string())?;
        let mut id = 0i64;
        for index in 0..MAX_PAGES {
            let offset = index * PAGE_ROWS;
            let page = read_page(dir, identity, offset)?;
            let rows = contract::validate_page(identity, offset, &page)?;
            let (size, modified) = page_stamp(dir, offset)?;
            transaction
                .execute(
                    "INSERT INTO page_manifest(page_offset, size, modified) VALUES (?1, ?2, ?3)",
                    params![offset as i64, size, modified],
                )
                .map_err(|e| e.to_string())?;
            for (feature_index, feature) in rows.iter().enumerate() {
                if let Some(bounds) = contract::feature_bounds(feature) {
                    transaction
                        .execute(
                            "INSERT INTO feature_locations(id, page_offset, feature_index) VALUES (?1, ?2, ?3)",
                            params![id, offset as i64, feature_index as i64],
                        )
                        .map_err(|e| e.to_string())?;
                    transaction
                        .execute(
                            "INSERT INTO feature_rtree(id, min_x, max_x, min_y, max_y) VALUES (?1, ?2, ?3, ?4, ?5)",
                            params![id, bounds[0], bounds[2], bounds[1], bounds[3]],
                        )
                        .map_err(|e| e.to_string())?;
                    id += 1;
                }
            }
            if page.next_offset.is_none() {
                break;
            }
        }
        transaction
            .execute(
                "INSERT INTO metadata(id, resource_id, version, feature_count, source_bytes) VALUES (1, ?1, ?2, ?3, ?4)",
                params![identity.resource_id, identity.version, summary.features as i64, summary.bytes as i64],
            )
            .map_err(|e| e.to_string())?;
        transaction.commit().map_err(|e| e.to_string())?;
        connection
            .execute_batch("PRAGMA optimize;")
            .map_err(|e| e.to_string())?;
        // Closed by name before the rename. On Windows an open handle makes the
        // rename fail outright; letting a `Connection` drop at the end of a
        // scope is not a statement that the file is closed, and a failed
        // `close` is a reason to keep the half-built file out of the way rather
        // than to promote it.
        connection.close().map_err(|(_, error)| error.to_string())?;
        Ok(())
    })();
    if let Err(error) = built {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    let target = dir.join(INDEX_FILE);
    if target.exists() {
        fs::remove_file(&target).map_err(|e| e.to_string())?;
    }
    if let Err(error) = fs::rename(&temp_path, &target) {
        let _ = fs::remove_file(&temp_path);
        return Err(error.to_string());
    }
    Ok(())
}

fn ensure_index(dir: &Path, identity: &Identity, summary: &CacheSummary) -> Result<(), String> {
    if index_valid(dir, identity, summary) {
        return Ok(());
    }
    build_index(dir, identity, summary)
}

fn installed_sizes(dir: &Path) -> Result<(u64, u64, u64), String> {
    let mut download_bytes = 0_u64;
    let mut index_bytes = 0_u64;
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() || linked(&metadata) {
            continue;
        }
        if entry.file_name() == INDEX_FILE || entry.file_name() == BUNDLE_INDEX_FILE {
            index_bytes += metadata.len();
        } else if entry
            .file_name()
            .to_string_lossy()
            .strip_suffix(".json")
            .is_some_and(|offset| offset.bytes().all(|byte| byte.is_ascii_digit()))
        {
            download_bytes = download_bytes.saturating_add(metadata.len());
        }
    }
    Ok((
        download_bytes,
        index_bytes,
        download_bytes.saturating_add(index_bytes),
    ))
}

fn installed_versions(root: &Path, resource_id: &str) -> Result<Value, String> {
    let identity = Identity {
        resource_id: resource_id.to_owned(),
        version: "0".repeat(64),
    };
    identity.validate()?;
    let resource_root = root
        .join("reference-query-cache")
        .join("v2")
        .join(resource_id);
    let metadata = match fs::symlink_metadata(&resource_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({"versions":[]}));
        }
        Err(error) => return Err(error.to_string()),
    };
    if linked(&metadata) || !metadata.is_dir() {
        return Err("invalid reference resource directory".into());
    }
    let mut versions = Vec::new();
    for entry in fs::read_dir(resource_root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        if name.len() == 64
            && name
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            && metadata.is_dir()
            && !linked(&metadata)
        {
            versions.push(name);
        }
    }
    versions.sort();
    Ok(json!({"versions":versions}))
}

fn query_index(
    dir: &Path,
    identity: &Identity,
    area: &Value,
    limit: usize,
) -> Result<Value, String> {
    contract::validate_area(area)?;
    let bounds = contract::area_bounds(area)?;
    if completed_bundle(dir, identity).is_some() {
        return query_bundle(dir, identity, area, bounds, limit);
    }
    if completed_summary(dir, identity).is_none() {
        return Err(
            "reference cache is incomplete or changed; verify or reinstall this resource".into(),
        );
    }
    let connection = open_index_for_read(&dir.join(INDEX_FILE)).map_err(|e| e.to_string())?;
    let mut statement = connection
        .prepare(
            "SELECT f.page_offset, f.feature_index
             FROM feature_rtree r
             JOIN feature_locations f ON f.id = r.id
             WHERE r.min_x <= ?1 AND r.max_x >= ?2
               AND r.min_y <= ?3 AND r.max_y >= ?4
             ORDER BY f.id",
        )
        .map_err(|e| e.to_string())?;
    let mut locations = statement
        .query(params![bounds[2], bounds[0], bounds[3], bounds[1]])
        .map_err(|e| e.to_string())?;
    let mut current_offset = None;
    let mut current_rows = Vec::new();
    let mut features = Vec::new();
    let mut bytes = 0usize;
    let mut truncated = false;
    while let Some(row) = locations.next().map_err(|e| e.to_string())? {
        let offset = row.get::<_, usize>(0).map_err(|e| e.to_string())?;
        let feature_index = row.get::<_, usize>(1).map_err(|e| e.to_string())?;
        if current_offset != Some(offset) {
            current_rows =
                contract::validate_page(identity, offset, &read_page(dir, identity, offset)?)?;
            current_offset = Some(offset);
        }
        let feature = current_rows
            .get(feature_index)
            .ok_or("reference spatial index points outside its page")?;
        if !contract::intersects_area(feature, area)? {
            continue;
        }
        if features.len() >= limit {
            truncated = true;
            break;
        }
        bytes = bytes.saturating_add(feature.to_string().len());
        if bytes > MAX_PAGE_BYTES {
            truncated = true;
            break;
        }
        features.push(feature.clone());
    }
    Ok(
        json!({"features":features,"truncated":truncated,"cache":"local","version":identity.version}),
    )
}

fn query_bundle(
    dir: &Path,
    identity: &Identity,
    area: &Value,
    bounds: [f64; 4],
    limit: usize,
) -> Result<Value, String> {
    let connection =
        open_index_for_read(&dir.join(BUNDLE_INDEX_FILE)).map_err(|e| e.to_string())?;
    let mut statement = connection
        .prepare(
            "SELECT f.feature_json FROM feature_rtree r JOIN features f ON f.id=r.id
        WHERE r.min_x<=?1 AND r.max_x>=?2 AND r.min_y<=?3 AND r.max_y>=?4 ORDER BY f.id",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = statement
        .query(params![bounds[2], bounds[0], bounds[3], bounds[1]])
        .map_err(|e| e.to_string())?;
    let mut features = Vec::new();
    let mut bytes = 0;
    let mut truncated = false;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let json: String = row.get(0).map_err(|e| e.to_string())?;
        let feature: Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        if !contract::intersects_area(&feature, area)? {
            continue;
        }
        if features.len() >= limit || bytes + json.len() > MAX_PAGE_BYTES {
            truncated = true;
            break;
        }
        bytes += json.len();
        features.push(feature);
    }
    Ok(
        json!({"features":features,"truncated":truncated,"cache":"local","version":identity.version}),
    )
}
/// The machine-shared geographic data root. The first call in a process also
/// tightens an install from before the privacy rule ([`crate::private`]).
pub fn default_root() -> Result<PathBuf, String> {
    let root = dirs::data_local_dir()
        .map(|p| p.join(OWNED_ROOT))
        .ok_or_else(|| "local reference cache directory is unavailable".to_string())?;
    private::tighten_root_once(&root);
    Ok(root)
}
pub fn configured_root() -> Result<PathBuf, String> {
    let default = default_root()?;
    let pointer = default.join(LOCATION_FILE);
    let bytes = match fs::read(&pointer) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(default),
        Err(error) => {
            return Err(format!(
                "could not read geographic data storage setting: {error}"
            ))
        }
    };
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "geographic data storage setting is invalid".to_string())?;
    let path = value["root"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "geographic data storage setting has no root".to_string())?;
    if !path.is_absolute() {
        return Err("geographic data storage root is not absolute".into());
    }
    // A relocated root is the owned directory DS made inside the operator's
    // chosen location; only that one is ever tightened, never the location.
    if path.file_name() == Some(std::ffi::OsStr::new(OWNED_ROOT)) {
        private::tighten_root_once(&path);
    }
    Ok(path)
}

pub fn download_staging_root() -> Result<PathBuf, String> {
    let root = configured_root()?;
    private::create_dir_all(&root).map_err(|error| error.to_string())?;
    let metadata = fs::symlink_metadata(&root).map_err(|error| error.to_string())?;
    if linked(&metadata) || !metadata.is_dir() {
        return Err("invalid geographic data storage root".into());
    }
    let staging = root.join("reference-bundle-staging");
    private::create_dir_all(&staging).map_err(|error| error.to_string())?;
    let metadata = fs::symlink_metadata(&staging).map_err(|error| error.to_string())?;
    if linked(&metadata) || !metadata.is_dir() {
        return Err("invalid geographic data staging directory".into());
    }
    Ok(staging)
}

fn storage_status(root: &Path) -> Result<Value, String> {
    private::create_dir_all(root)
        .map_err(|error| format!("could not create geographic data storage: {error}"))?;
    let metadata = fs::symlink_metadata(root).map_err(|error| error.to_string())?;
    if linked(&metadata) || !metadata.is_dir() {
        return Err("geographic data storage is linked or is not a directory".into());
    }
    let available = fs2::available_space(root)
        .map_err(|error| format!("could not read available disk space: {error}"))?;
    let total = fs2::total_space(root)
        .map_err(|error| format!("could not read total disk space: {error}"))?;
    Ok(json!({
        "root": root.to_string_lossy(),
        "available_bytes": available,
        "total_bytes": total,
        "recommended": root == default_root()?.as_path(),
    }))
}

fn set_storage_root(path: Option<&str>) -> Result<Value, String> {
    let default = default_root()?;
    private::create_dir_all(&default).map_err(|error| error.to_string())?;
    let pointer = default.join(LOCATION_FILE);
    let root = if let Some(raw) = path.filter(|value| !value.trim().is_empty()) {
        let base = PathBuf::from(raw.trim());
        if !base.is_absolute() {
            return Err("geographic data storage location must be absolute".into());
        }
        let metadata = fs::symlink_metadata(&base)
            .map_err(|_| "choose an existing geographic data storage directory".to_string())?;
        if linked(&metadata) || !metadata.is_dir() {
            return Err("geographic data storage location must be a plain directory".into());
        }
        let canonical = fs::canonicalize(base).map_err(|error| error.to_string())?;
        let owned = canonical.join(OWNED_ROOT);
        private::create_dir_all(&owned).map_err(|error| {
            format!("could not create owned geographic data directory: {error}")
        })?;
        let bytes = serde_json::to_vec(&json!({"root": owned.to_string_lossy()}))
            .map_err(|error| error.to_string())?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(&default).map_err(|error| error.to_string())?;
        temporary
            .write_all(&bytes)
            .map_err(|error| error.to_string())?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| error.to_string())?;
        temporary
            .persist(&pointer)
            .map_err(|error| error.error.to_string())?;
        owned
    } else {
        match fs::remove_file(&pointer) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.to_string()),
        }
        default
    };
    storage_status(&root)
}

pub fn execute(request: Value) -> Result<Value, String> {
    match request["action"].as_str().unwrap_or("") {
        "storage_status" => storage_status(&configured_root()?),
        "set_storage_root" => set_storage_root(request["path"].as_str()),
        "versions" => {
            let resource_id = request["identity"]["resource_id"]
                .as_str()
                .ok_or("reference resource id is required")?;
            installed_versions(&configured_root()?, resource_id)
        }
        _ => execute_at(&configured_root()?, request),
    }
}
pub fn execute_at(root: &Path, request: Value) -> Result<Value, String> {
    let identity: Identity = serde_json::from_value(request["identity"].clone())
        .map_err(|_| "reference identity is required")?;
    let action = request["action"].as_str().unwrap_or("");
    let lock = resource_lock(root, &identity)?;
    // Queries share a lock. Install/remove affect only this resource, so a large
    // download cannot freeze the status of every other dataset on the computer.
    if matches!(action, "query" | "status") {
        FileExt::lock_shared(&lock).map_err(|e| e.to_string())?;
    } else {
        lock.lock_exclusive().map_err(|e| e.to_string())?;
    }
    let dir = directory(root, &identity)?;
    // Upgrade older indexes once, under a writer lock. Completed installs never
    // take this path and never rescan their source pages on a status refresh.
    if action == "status" && completed_summary(&dir, &identity).is_none() {
        FileExt::unlock(&lock).map_err(|e| e.to_string())?;
        lock.lock_exclusive().map_err(|e| e.to_string())?;
    }
    execute_locked(&directory(root, &identity)?, &identity, request)
}

fn resource_lock(root: &Path, identity: &Identity) -> Result<fs::File, String> {
    identity.validate()?;
    private::create_dir_all(root).map_err(|e| e.to_string())?;
    if linked(&fs::symlink_metadata(root).map_err(|e| e.to_string())?) {
        return Err("linked reference root is not allowed".into());
    }
    // Lives outside the removable version directories; all versions of one
    // resource serialize publication/removal against their active readers.
    let lock_path = root.join(format!(
        "reference-query-cache-{}.lock",
        identity.resource_id
    ));
    if let Ok(md) = fs::symlink_metadata(&lock_path) {
        if linked(&md) || !md.is_file() {
            return Err("invalid reference cache lock".into());
        }
    }
    let lock = private::open_options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|e| e.to_string())?;
    Ok(lock)
}

fn execute_locked(dir: &Path, identity: &Identity, request: Value) -> Result<Value, String> {
    match request["action"].as_str().unwrap_or("") {
        "store" => {
            let offset = usize::try_from(
                request["offset"]
                    .as_u64()
                    .ok_or("page offset is required")?,
            )
            .map_err(|_| "page offset exceeds platform limit")?;
            let page: Page = serde_json::from_value(request["page"].clone())
                .map_err(|_| "reference page is required")?;
            // A page carrying a geometry DS does not admit is never cached.
            let rows = contract::validate_page(identity, offset, &page)?;
            for (index, feature) in rows.iter().enumerate() {
                admit_reference_feature(offset + index + 1, feature)?;
            }
            let target = dir.join(format!("{offset}.json"));
            if target.exists() {
                let held = read_page(dir, identity, offset)?;
                if held.sha256 != page.sha256 {
                    return Err(
                        "reference cache version has conflicting bytes; remove and reinstall"
                            .into(),
                    );
                }
                return Ok(json!({"stored":true}));
            }
            let bytes = serde_json::to_vec(&page).map_err(|e| e.to_string())?;
            let mut size = bytes.len() as u64;
            for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
                size = size.saturating_add(
                    entry
                        .map_err(|e| e.to_string())?
                        .metadata()
                        .map_err(|e| e.to_string())?
                        .len(),
                );
            }
            if size > MAX_RESOURCE_BYTES {
                return Err("reference cache exceeds its 64 GiB resource limit".into());
            }
            let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
            temp.write_all(&bytes).map_err(|e| e.to_string())?;
            temp.as_file().sync_all().map_err(|e| e.to_string())?;
            if temp.persist_noclobber(target).is_err() {
                let held = read_page(dir, identity, offset)?;
                if held.sha256 != page.sha256 {
                    return Err("concurrent reference install conflict".into());
                }
            }
            if page.next_offset.is_none() {
                let summary = scan_pages(dir, identity)?;
                if summary.terminal {
                    ensure_index(dir, identity, &summary)?;
                }
            }
            Ok(json!({"stored":true}))
        }
        "status" => {
            let summary = match completed_summary(dir, identity) {
                Some(summary) => summary,
                None => scan_pages(dir, identity)?,
            };
            if !summary.terminal {
                let (download_bytes, index_bytes, disk_bytes) = installed_sizes(dir)?;
                return Ok(
                    json!({"ready":false,"features":summary.features,"bytes":summary.bytes,"download_bytes":download_bytes,"index_bytes":index_bytes,"disk_bytes":disk_bytes,"message":summary.message}),
                );
            }
            ensure_index(dir, identity, &summary)?;
            let (download_bytes, index_bytes, disk_bytes) = installed_sizes(dir)?;
            Ok(
                json!({"ready":true,"features":summary.features,"bytes":summary.bytes,"download_bytes":download_bytes,"index_bytes":index_bytes,"disk_bytes":disk_bytes,"indexed":true}),
            )
        }
        "query" => {
            let area = request
                .get("area")
                .filter(|value| !value.is_null())
                .ok_or("query area is required")?;
            contract::validate_area(area)?;
            let limit = request["limit"].as_u64().unwrap_or(5000).clamp(1, 10000) as usize;
            query_index(dir, identity, area, limit)
        }
        "remove_all_versions" => {
            fs::remove_dir_all(dir.parent().ok_or("invalid resource directory")?)
                .map_err(|e| e.to_string())?;
            Ok(json!({"removed":true}))
        }
        "remove" => {
            fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
            Ok(json!({"removed":true}))
        }
        _ => Err("unsupported reference cache operation".into()),
    }
}

/// Promote an already verified, indexed bundle on the same filesystem. Nothing
/// in the private staging root is discoverable by ordinary status/query calls.
pub fn publish_staged(
    root: &Path,
    staging_root: &Path,
    identity: &Identity,
) -> Result<Value, String> {
    publish_staged_at(root, staging_root, identity)
}

fn publish_staged_at(
    root: &Path,
    staging_root: &Path,
    identity: &Identity,
) -> Result<Value, String> {
    let staged = directory(staging_root, identity)?;
    if completed_summary(&staged, identity).is_none() {
        return Err("cannot publish an incomplete reference index".into());
    }
    let lock = resource_lock(root, identity)?;
    lock.lock_exclusive().map_err(|e| e.to_string())?;
    let target = directory(root, identity)?;
    if completed_summary(&target, identity).is_none() {
        let retired = tempfile::tempdir_in(root).map_err(|e| e.to_string())?;
        let previous = retired.path().join("previous");
        fs::rename(&target, &previous).map_err(|e| e.to_string())?;
        if let Err(error) = fs::rename(&staged, &target) {
            fs::rename(&previous, &target).map_err(|rollback| {
                format!("publish failed: {error}; restore failed: {rollback}")
            })?;
            return Err(error.to_string());
        }
    }
    execute_locked(&target, identity, json!({"action":"status"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    fn request(action: &str) -> Value {
        json!({"action":action,"identity":{"resource_id":"a".repeat(64),"version":"b".repeat(64)}})
    }
    fn page(count: usize, offset: usize, more: bool) -> Value {
        let feature = json!({"type":"Feature","properties":{"name":"école"},"geometry":{"type":"Point","coordinates":[30.,-2.]}});
        let payload = serde_json::to_string(&vec![feature; count]).unwrap();
        json!({"resource_id":"a".repeat(64),"version":"b".repeat(64),"offset":offset,"area":null,"feature_json":payload,"feature_count":count,"sha256":format!("{:x}",Sha256::digest(payload.as_bytes())),"next_offset":if more {Some(offset+PAGE_ROWS)} else {None}})
    }
    fn one_feature_page(feature: Value) -> Value {
        let payload = serde_json::to_string(&vec![feature]).unwrap();
        json!({"resource_id":"a".repeat(64),"version":"b".repeat(64),"offset":0,"area":null,"feature_json":payload,"feature_count":1,"sha256":format!("{:x}",Sha256::digest(payload.as_bytes())),"next_offset":null})
    }
    fn install(root: &Path, offset: usize, page: Value) -> Result<Value, String> {
        let mut req = request("store");
        req["offset"] = json!(offset);
        req["page"] = page;
        execute_at(root, req)
    }
    #[test]
    fn a_reference_page_carrying_a_geometry_ds_does_not_admit_is_never_cached() {
        let root = tempfile::tempdir().unwrap();
        let inverted = json!({"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[
            [[180.,90.],[-180.,90.],[-180.,-90.],[180.,-90.],[180.,90.]],
            [[30.,-2.],[30.1,-2.],[30.1,-2.1],[30.,-2.1],[30.,-2.]]]}});
        let refused = install(root.path(), 0, one_feature_page(inverted)).unwrap_err();
        assert!(
            refused.starts_with("bundle_invalid: feature 1 polygon_covers_hemisphere: "),
            "{refused}"
        );
        assert_eq!(
            execute_at(root.path(), request("status")).unwrap()["ready"],
            false
        );
    }
    #[test]
    fn interrupted_download_resumes_and_queries_after_reopen() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            execute_at(root.path(), request("status")).unwrap()["ready"],
            false
        );
        let first = page(PAGE_ROWS, 0, true);
        install(root.path(), 0, first.clone()).unwrap();
        assert_eq!(
            execute_at(root.path(), request("status")).unwrap()["ready"],
            false
        );
        // Retry is idempotent and a final page completes the exact version.
        install(root.path(), 0, first).unwrap();
        install(root.path(), 1000, page(1, 1000, false)).unwrap();
        assert_eq!(
            execute_at(root.path(), request("status")).unwrap()["features"],
            1001
        );
        let mut query = request("query");
        query["area"] = json!({"type":"Polygon","coordinates":[[[29.,-3.],[31.,-3.],[31.,-1.],[29.,-1.],[29.,-3.]]]});
        query["limit"] = json!(10);
        let result = execute_at(root.path(), query.clone()).unwrap();
        assert_eq!(result["features"].as_array().unwrap().len(), 10);
        assert_eq!(result["truncated"], true);
        query["area"] =
            json!({"type":"Polygon","coordinates":[[[0.,0.],[1.,0.],[1.,1.],[0.,1.],[0.,0.]]]});
        assert_eq!(
            execute_at(root.path(), query).unwrap()["features"],
            json!([])
        );
        let mut other = request("status");
        other["identity"]["version"] = json!("c".repeat(64));
        assert_eq!(execute_at(root.path(), other).unwrap()["ready"], false);
        execute_at(root.path(), request("remove")).unwrap();
        assert_eq!(
            execute_at(root.path(), request("status")).unwrap()["ready"],
            false
        );
    }

    #[test]
    fn installed_versions_are_digest_bounded_and_do_not_create_a_candidate() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            installed_versions(root.path(), &"a".repeat(64)).unwrap()["versions"],
            json!([])
        );
        install(root.path(), 0, page(1, 0, false)).unwrap();
        assert_eq!(
            installed_versions(root.path(), &"a".repeat(64)).unwrap()["versions"],
            json!(["b".repeat(64)])
        );
    }
    #[test]
    fn corruption_area_queries_and_conflicting_bytes_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let mut bounded = page(1, 0, false);
        bounded["area"] = json!({"type":"Polygon","coordinates":[[[29.,-3.],[31.,-3.],[31.,-1.],[29.,-1.],[29.,-3.]]]});
        assert!(install(root.path(), 0, bounded).is_err());
        install(root.path(), 0, page(1, 0, false)).unwrap();
        assert!(install(root.path(), 0, page(0, 0, false)).is_err());
        let identity: Identity =
            serde_json::from_value(request("status")["identity"].clone()).unwrap();
        fs::write(
            directory(root.path(), &identity).unwrap().join("0.json"),
            b"{}",
        )
        .unwrap();
        assert_eq!(
            execute_at(root.path(), request("status")).unwrap()["ready"],
            false
        );
        let mut query = request("query");
        query["area"] = json!({"type":"Polygon","coordinates":[[[29.,-3.],[31.,-3.],[31.,-1.],[29.,-1.],[29.,-3.]]]});
        assert!(execute_at(root.path(), query).is_err());
    }
    #[test]
    fn completed_install_builds_rtree_and_exact_area_removes_hole_candidates() {
        let root = tempfile::tempdir().unwrap();
        install(
            root.path(),
            0,
            one_feature_page(json!({"type":"Feature","properties":{"name":"inside envelope, inside hole"},"geometry":{"type":"Point","coordinates":[1.,1.]}})),
        )
        .unwrap();
        let identity: Identity =
            serde_json::from_value(request("status")["identity"].clone()).unwrap();
        let dir = directory(root.path(), &identity).unwrap();
        assert!(dir.join(INDEX_FILE).is_file());
        let mut query = request("query");
        query["area"] = json!({"type":"Polygon","coordinates":[[[0.,0.],[2.,0.],[2.,2.],[0.,2.],[0.,0.]],[[0.5,0.5],[1.5,0.5],[1.5,1.5],[0.5,1.5],[0.5,0.5]]]});
        let result = execute_at(root.path(), query).unwrap();
        assert_eq!(result["features"], json!([]));
        assert_eq!(result["cache"], "local");
        #[cfg(unix)]
        crate::private::assert_private_tree(root.path());
    }
    #[test]
    fn a_bundle_feature_ds_does_not_admit_is_refused_by_name() {
        let root = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir_in(root.path()).unwrap();
        let identity: Identity =
            serde_json::from_value(request("status")["identity"].clone()).unwrap();
        let mut writer = BundleIndexWriter::new(staging.path(), identity).unwrap();
        writer
            .push(&json!({"type":"Feature","properties":{},"geometry":null}))
            .unwrap();
        writer
            .push(&json!({"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[30.,-2.]}}))
            .unwrap();
        let inverted = json!({"type":"Feature","id":"lake-7","properties":{},"geometry":{"type":"Polygon","coordinates":[
            [[180.,90.],[-180.,90.],[-180.,-90.],[180.,-90.],[180.,90.]],
            [[30.,-2.],[30.1,-2.],[30.1,-2.1],[30.,-2.1],[30.,-2.]]]}});
        let refused = writer.push(&inverted).unwrap_err();
        assert!(
            refused.starts_with(
                "bundle_invalid: feature 3 (id \"lake-7\") polygon_covers_hemisphere: Polygon ring 0"
            ),
            "{refused}"
        );
        let bowtie = json!({"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[[[0.,0.],[2.,2.],[2.,0.],[0.,2.],[0.,0.]]]}});
        assert_eq!(
            writer.push(&bowtie).unwrap_err(),
            "bundle_invalid: feature 3 geometry_invalid: Polygon ring 0 crosses itself: edges 0 \
             and 2 cross at [1, 1]"
        );
    }
    #[test]
    fn native_bundle_can_exceed_cloud_page_bytes_without_materializing_a_page() {
        let root = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir_in(root.path()).unwrap();
        let identity: Identity =
            serde_json::from_value(request("status")["identity"].clone()).unwrap();
        let feature = json!({"type":"Feature","properties":{"text":"x".repeat(32*1024)},"geometry":{"type":"Point","coordinates":[30.,-2.]}});
        let mut writer = BundleIndexWriter::new(staging.path(), identity.clone()).unwrap();
        for _ in 0..600 {
            writer.push(&feature).unwrap();
        }
        // Even a complete set of rows stays private until final verification.
        assert!(
            completed_bundle(&directory(staging.path(), &identity).unwrap(), &identity).is_none()
        );
        writer
            .finish((600 * feature.to_string().len()) as u64)
            .unwrap();
        let status = publish_staged_at(root.path(), staging.path(), &identity).unwrap();
        assert_eq!(status["features"], 600);
        assert!(status["bytes"].as_u64().unwrap() > MAX_PAGE_BYTES as u64);
        let mut query = request("query");
        query["area"] = json!({"type":"Polygon","coordinates":[[[29.,-3.],[31.,-3.],[31.,-1.],[29.,-1.],[29.,-3.]]]});
        query["limit"] = json!(1);
        let result = execute_at(root.path(), query).unwrap();
        assert_eq!(result["features"].as_array().unwrap().len(), 1);
        assert_eq!(result["truncated"], true);
    }

    /// A root shaped like the one a Windows install actually stages under, of a
    /// stated minimum length. Nothing here is Windows-specific except the
    /// arithmetic it reproduces: the staging root, the per-bundle directory
    /// named by two 64-character digests, and a temporary directory inside it.
    fn long_staging_root(base: &Path, minimum: usize) -> PathBuf {
        let mut root = base.join("rw.datasolutions.desktop.shared");
        root.push("reference-bundle-staging");
        root.push(format!(
            "reference-bundle-{}-{}",
            "a".repeat(64),
            "b".repeat(64)
        ));
        root.push("staged");
        while root.as_os_str().len() < minimum {
            root.push("dsdsdsdsdsdsdsdsdsdsdsdsdsdsdsdsdsdsdsdsdsdsdsdsdsdsdsdsdsdsdsds");
        }
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// The lifecycle a Windows reference install runs, at Windows path lengths.
    ///
    /// `download_staging_root` → a private staging directory → a streamed
    /// bundle index → promotion → status and query from the live root. The
    /// index database itself lands several segments deeper than the root this
    /// starts from (`reference-query-cache/v2/<64 hex>/<64 hex>/`), which is
    /// what carries the real install past Windows' 260-character path limit —
    /// the limit `fs` hides from Rust and SQLite does not.
    #[test]
    fn staged_install_completes_at_windows_path_lengths() {
        let base = tempfile::tempdir().unwrap();
        let identity: Identity =
            serde_json::from_value(request("status")["identity"].clone()).unwrap();
        for minimum in [200_usize, 260] {
            let live = base.path().join(format!("live-{minimum}"));
            fs::create_dir_all(&live).unwrap();
            let staging = long_staging_root(&base.path().join(format!("s{minimum}")), minimum);
            assert!(staging.as_os_str().len() >= minimum);
            let indexed = directory(&staging, &identity).unwrap();
            assert!(
                indexed.join(BUNDLE_INDEX_FILE).as_os_str().len() > 260,
                "the database path is what has to be long, not the root",
            );
            let feature = json!({"type":"Feature","properties":{"name":"long"},"geometry":{"type":"Point","coordinates":[30.,-2.]}});
            let mut writer = BundleIndexWriter::new(&staging, identity.clone()).unwrap();
            writer.push(&feature).unwrap();
            writer.finish(feature.to_string().len() as u64).unwrap();
            let status = publish_staged_at(&live, &staging, &identity).unwrap();
            assert_eq!(status["ready"], true);
            assert_eq!(status["features"], 1);
            let mut query = request("query");
            query["area"] = json!({"type":"Polygon","coordinates":[[[29.,-3.],[31.,-3.],[31.,-1.],[29.,-1.],[29.,-3.]]]});
            let result = execute_at(&live, query).unwrap();
            assert_eq!(result["features"].as_array().unwrap().len(), 1);
        }
    }

    /// The page-cache half of the same lifecycle: the RTree index is BUILT, not
    /// streamed, and its build file is promoted by rename. Both the build and
    /// the promotion happen at a Windows-length path here.
    #[test]
    fn page_index_builds_and_promotes_at_windows_path_lengths() {
        let base = tempfile::tempdir().unwrap();
        let root = long_staging_root(base.path(), 200);
        install(&root, 0, page(2, 0, false)).unwrap();
        let identity: Identity =
            serde_json::from_value(request("status")["identity"].clone()).unwrap();
        let dir = directory(&root, &identity).unwrap();
        assert!(dir.join(INDEX_FILE).as_os_str().len() > 260);
        assert!(dir.join(INDEX_FILE).is_file());
        // Nothing half-built is left behind to be reopened by the next run.
        assert!(!dir.join(INDEX_BUILD_FILE).exists());
        assert_eq!(execute_at(&root, request("status")).unwrap()["ready"], true);
    }

    /// A half-written index from an interrupted run is replaced, not resumed.
    #[test]
    fn an_abandoned_build_file_never_becomes_the_index() {
        let root = tempfile::tempdir().unwrap();
        let identity: Identity =
            serde_json::from_value(request("status")["identity"].clone()).unwrap();
        let dir = directory(root.path(), &identity).unwrap();
        fs::write(dir.join(INDEX_BUILD_FILE), b"not a database at all").unwrap();
        install(root.path(), 0, page(1, 0, false)).unwrap();
        assert!(!dir.join(INDEX_BUILD_FILE).exists());
        assert_eq!(
            execute_at(root.path(), request("status")).unwrap()["ready"],
            true
        );
    }

    /// Digest verification of an installed bundle is unchanged by any of this:
    /// a published index still has to agree with the identity it claims.
    #[test]
    fn a_published_index_still_proves_its_identity() {
        let root = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir_in(root.path()).unwrap();
        let identity: Identity =
            serde_json::from_value(request("status")["identity"].clone()).unwrap();
        let other = Identity {
            resource_id: "c".repeat(64),
            version: "d".repeat(64),
        };
        let feature = json!({"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[30.,-2.]}});
        let mut writer = BundleIndexWriter::new(staging.path(), other.clone()).unwrap();
        writer.push(&feature).unwrap();
        writer.finish(feature.to_string().len() as u64).unwrap();
        // The staged index belongs to another resource, so this one is absent.
        assert!(publish_staged_at(root.path(), staging.path(), &identity).is_err());
    }

    #[test]
    fn a_sqlite_path_is_never_resolved_through_a_link() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("spatial-index.sqlite");
        let prepared = crate::sqlite_path(&path).unwrap();
        #[cfg(windows)]
        {
            let text = prepared.to_string_lossy().into_owned();
            assert!(text.starts_with(r"\\?\"), "{text}");
            assert!(text.ends_with("spatial-index.sqlite"), "{text}");
            // Prefixing twice would be a path no API accepts, so an already
            // prefixed path is returned as it is.
            assert_eq!(crate::sqlite_path(&prepared).unwrap(), prepared);
            assert_eq!(text.matches(r"?\").count(), 1, "{text}");
        }
        #[cfg(not(windows))]
        assert_eq!(prepared, path);
    }

    #[test]
    fn staged_download_is_invisible_until_publication_and_failed_retry_keeps_live_data() {
        let root = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir_in(root.path()).unwrap();
        let identity: Identity =
            serde_json::from_value(request("status")["identity"].clone()).unwrap();
        install(staging.path(), 0, page(1, 0, false)).unwrap();
        assert_eq!(
            execute_at(root.path(), request("status")).unwrap()["ready"],
            false
        );
        assert_eq!(
            publish_staged_at(root.path(), staging.path(), &identity).unwrap()["ready"],
            true
        );
        let broken = tempfile::tempdir_in(root.path()).unwrap();
        install(broken.path(), 0, page(PAGE_ROWS, 0, true)).unwrap();
        assert!(publish_staged_at(root.path(), broken.path(), &identity).is_err());
        assert_eq!(
            execute_at(root.path(), request("status")).unwrap()["features"],
            1
        );
    }

    #[test]
    fn another_resource_remains_readable_during_install_and_reads_share_locks() {
        let root = tempfile::tempdir().unwrap();
        install(root.path(), 0, page(1, 0, false)).unwrap();
        let identity: Identity =
            serde_json::from_value(request("status")["identity"].clone()).unwrap();
        let busy = Identity {
            resource_id: "c".repeat(64),
            version: identity.version.clone(),
        };
        let writer = resource_lock(root.path(), &busy).unwrap();
        writer.lock_exclusive().unwrap();
        let reader = resource_lock(root.path(), &identity).unwrap();
        reader.try_lock_shared().unwrap();
        assert_eq!(
            execute_at(root.path(), request("status")).unwrap()["ready"],
            true
        );
    }

    #[test]
    fn older_index_is_upgraded_once_without_redownloading_pages() {
        let root = tempfile::tempdir().unwrap();
        install(root.path(), 0, page(1, 0, false)).unwrap();
        let identity: Identity =
            serde_json::from_value(request("status")["identity"].clone()).unwrap();
        let dir = directory(root.path(), &identity).unwrap();
        Connection::open(dir.join(INDEX_FILE))
            .unwrap()
            .execute_batch("DROP TABLE page_manifest")
            .unwrap();
        assert!(completed_summary(&dir, &identity).is_none());
        assert_eq!(
            execute_at(root.path(), request("status")).unwrap()["ready"],
            true
        );
        assert!(completed_summary(&dir, &identity).is_some());
    }

    #[test]
    fn traversal_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let mut req = request("remove");
        req["identity"]["resource_id"] = json!("../other");
        assert!(execute_at(root.path(), req).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn linked_cache_cannot_escape_shared_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("reference-query-cache"))
            .unwrap();
        assert!(execute_at(root.path(), request("remove")).is_err());
        assert!(outside.path().exists());
    }
}
