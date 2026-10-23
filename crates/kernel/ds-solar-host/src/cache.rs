//! The versioned local reference-bundle cache.
//!
//! The cache is what makes a completely offline run possible, so its contract
//! is narrow and strict: an entry is returned only if every artifact in it
//! verifies against its own manifest, and one that no longer verifies is
//! treated as **absent** rather than repaired. The host owns the root
//! directory — the engine never sees a path.
//!
//! Entries are keyed by [`ReferenceKey`], which carries the pin, so a provider
//! or contract change cannot collide with an entry written under the previous
//! one. Nothing here expires. A reference unit is a constant of its
//! coordinates, local PV request and pins, and refreshing it is an explicit
//! act — `ds solar prepare --overwrite`, or an operator refresh in the app —
//! never a time-to-live decision taken during a run.
//!
//! Note what a client *cannot* do offline: decide that an entry is stale. The
//! producer's `source_key` also covers the pvlib version, the resolved catalog
//! rows and every loss parameter, and none of those are knowable without
//! asking. That asymmetry is honest and it is why the refresh is a flag rather
//! than an inference.

use fs2::FileExt;
use std::io::{Read, Write};
use std::path::PathBuf;

use ds_solar_contracts::{BundleBytes, ReferenceKey, SolarError, SolarResult};

/// Cache layout version. v3 adds the local reference-request digest to each
/// key, retiring unsound coordinate-only v2 entries without deleting user
/// data. A panel/loss/capacity change must never reuse its predecessor's
/// sealed reference unit.
pub const CACHE_LAYOUT_VERSION: &str = "v3";

/// The four artifacts, in the order they are listed everywhere else.
pub const ARTIFACT_NAMES: [&str; 4] = [
    "weather.json",
    "reference-unit.json",
    "reference-unit.parquet",
    "manifest.json",
];

/// A place cached reference bundles live.
pub trait ReferenceCache: Send + Sync {
    /// Look one up. Returns `Ok(None)` for a miss and for an entry that no
    /// longer verifies.
    fn get(&self, key: &ReferenceKey) -> SolarResult<Option<BundleBytes>>;

    /// Store one. Overwrites any existing entry for the key.
    fn put(&self, key: &ReferenceKey, artifacts: &BundleBytes) -> SolarResult<()>;
}

/// A cache that lives only as long as the process. Used by the stateless Cloud
/// Run adapter and by tests.
#[derive(Debug, Default)]
pub struct InMemoryReferenceCache {
    entries: std::sync::RwLock<std::collections::HashMap<String, BundleBytes>>,
}

impl InMemoryReferenceCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many entries it holds.
    pub fn len(&self) -> usize {
        self.entries
            .read()
            .map(|entries| entries.len())
            .unwrap_or(0)
    }

    /// Whether it holds nothing.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl ReferenceCache for InMemoryReferenceCache {
    fn get(&self, key: &ReferenceKey) -> SolarResult<Option<BundleBytes>> {
        let entries = self
            .entries
            .read()
            .map_err(|_| SolarError::reference_unit_invalid("reference cache lock is poisoned"))?;
        Ok(entries
            .get(&key.cache_entry())
            .cloned()
            .filter(|artifacts| crate::reference::open(artifacts).is_ok()))
    }

    fn put(&self, key: &ReferenceKey, artifacts: &BundleBytes) -> SolarResult<()> {
        crate::reference::open(artifacts)?;
        let mut entries = self
            .entries
            .write()
            .map_err(|_| SolarError::reference_unit_invalid("reference cache lock is poisoned"))?;
        entries.insert(key.cache_entry(), artifacts.clone());
        Ok(())
    }
}

/// A cache backed by a directory the host chose.
///
/// The desktop shell and the CLI own workspace and cache locations, so they
/// pass the root in. Nothing in this type discovers, defaults or guesses a
/// path.
#[derive(Debug, Clone)]
pub struct DirectoryReferenceCache {
    root: PathBuf,
}

impl DirectoryReferenceCache {
    /// Use `root` as the cache root. Entries are written under a
    /// version-named subdirectory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The directory this cache writes into.
    pub fn versioned_root(&self) -> PathBuf {
        self.root.join(CACHE_LAYOUT_VERSION)
    }

    /// The directory one bundle's four artifacts live in.
    fn base_dir(&self, key: &ReferenceKey) -> PathBuf {
        // A cache entry name carries `/` separators and provider text; flatten
        // it so one key is one directory and a key can never escape the root.
        let flat: String = key
            .cache_entry()
            .chars()
            .map(|character| match character {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => character,
                _ => '_',
            })
            .collect();
        self.versioned_root().join(flat)
    }
    /// The last fully committed generation. Older v3 entries remain readable
    /// until a verified generation is committed; they are never deleted.
    pub fn entry_dir(&self, key: &ReferenceKey) -> PathBuf {
        let base = self.base_dir(key);
        let pointer = base.join("CURRENT");
        let read_pointer = || -> std::io::Result<String> {
            let meta = std::fs::symlink_metadata(&pointer)?;
            if !meta.is_file() || meta.len() > 96 {
                return Err(std::io::Error::other("Invalid cache generation pointer"));
            }
            let mut value = String::new();
            std::fs::File::open(pointer)?
                .take(97)
                .read_to_string(&mut value)?;
            Ok(value)
        };
        match read_pointer() {
            Ok(generation)
                if generation.len() <= 96
                    && generation.len() >= 64
                    && generation.as_bytes()[..64]
                        .iter()
                        .copied()
                        .all(|b| b.is_ascii_hexdigit())
                    && generation
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-') =>
            {
                base.join(generation)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => base,
            _ => base.join("invalid-generation"),
        }
    }
}

impl ReferenceCache for DirectoryReferenceCache {
    fn get(&self, key: &ReferenceKey) -> SolarResult<Option<BundleBytes>> {
        let directory = self.entry_dir(key);
        let mut bodies: Vec<Vec<u8>> = Vec::with_capacity(ARTIFACT_NAMES.len());
        for name in ARTIFACT_NAMES {
            // An unreadable artifact is a miss, not a failure: preparation can
            // still fetch and rewrite the entry.
            let path = directory.join(name);
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                return Ok(None);
            };
            if !meta.is_file() || meta.len() > crate::MAX_REFERENCE_ARTIFACT_BYTES {
                return Ok(None);
            }
            let mut body = Vec::new();
            if std::fs::File::open(path)
                .and_then(|f| {
                    f.take(crate::MAX_REFERENCE_ARTIFACT_BYTES + 1)
                        .read_to_end(&mut body)
                })
                .is_err()
                || body.len() as u64 > crate::MAX_REFERENCE_ARTIFACT_BYTES
            {
                return Ok(None);
            }
            bodies.push(body);
        }
        let mut bodies = bodies.into_iter();
        let artifacts = BundleBytes {
            weather_json: bodies.next().expect("four artifacts"),
            reference_unit_json: bodies.next().expect("four artifacts"),
            reference_unit_parquet: bodies.next().expect("four artifacts"),
            manifest_json: bodies.next().expect("four artifacts"),
        };
        Ok(Some(artifacts).filter(|artifacts| crate::reference::open(artifacts).is_ok()))
    }

    fn put(&self, key: &ReferenceKey, artifacts: &BundleBytes) -> SolarResult<()> {
        let opened = crate::reference::open(artifacts)?;
        let generation = opened
            .reference_unit
            .manifest
            .bundle_digest
            .trim_start_matches("sha256:");
        if generation.len() != 64 || !generation.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(SolarError::reference_unit_invalid(
                "Invalid cache generation digest",
            ));
        }
        let base = self.base_dir(key);
        let commit = || -> std::io::Result<()> {
            std::fs::create_dir_all(&base)?;
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(base.join("writer.lock"))?;
            lock.lock_exclusive()?;
            let staged = tempfile::Builder::new()
                .prefix(&format!("{generation}-"))
                .tempdir_in(&base)?;
            for (name, body) in [
                ("weather.json", &artifacts.weather_json),
                ("reference-unit.json", &artifacts.reference_unit_json),
                ("reference-unit.parquet", &artifacts.reference_unit_parquet),
                ("manifest.json", &artifacts.manifest_json),
            ] {
                if body.len() as u64 > crate::MAX_REFERENCE_ARTIFACT_BYTES {
                    return Err(std::io::Error::other(
                        "Reference artifact exceeds size bound",
                    ));
                }
                let mut file = std::fs::File::create(staged.path().join(name))?;
                file.write_all(body)?;
                file.sync_all()?;
            }
            #[cfg(unix)]
            std::fs::File::open(staged.path())?.sync_all()?;
            let committed = staged.keep();
            let generation = committed
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| std::io::Error::other("Invalid cache generation name"))?;
            // One pointer commits all four immutable artifacts. A crash before
            // this replacement leaves the previous generation fully readable.
            let mut pointer = tempfile::NamedTempFile::new_in(&base)?;
            pointer.write_all(generation.as_bytes())?;
            pointer.as_file().sync_all()?;
            pointer.persist(base.join("CURRENT")).map_err(|e| e.error)?;
            #[cfg(unix)]
            std::fs::File::open(&base)?.sync_all()?;
            Ok(())
        };
        commit().map_err(|e| {
            SolarError::reference_unit_invalid(format!("Reference cache commit failed: {e}"))
        })
    }
}
